//! Bounded intake with path coalescing, generation map, and priority lanes.
//!
//! The walk submits one item per matched file instead of spawning a task per
//! file; the watcher debounce and the expire looper submit through the same
//! intake, and request-path album updates arrive on the Interactive lane and
//! report back through a oneshot. A fixed set of workers (see [`super::worker`])
//! dequeues items in smooth weighted round-robin lane order (see
//! [`super::priority`]) and a per-path generation lets a newer submit
//! supersede an older queued item across lanes: re-submitting a path bumps
//! its generation, and the worker skips an item whose generation is stale.
//!
//! `submit_scan` records the file identity observed at enqueue (`mtime` +
//! `len`) and bumps the per-path generation. The worker skips an item whose
//! generation has been superseded by a newer submit, or whose file changed
//! since enqueue; both cases complete the per-job barrier without touching
//! the scan counters. A submit for a path that no longer stats is still
//! enqueued for `Scan` so `process_file` reports it as a failed file,
//! exactly as the per-file tasks did; `WatcherRemove` items skip the
//! identity check because the missing file is the point.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::SystemTime;

use anyhow::{Context, Result};
use arrayvec::ArrayString;
use dashmap::DashMap;
use tokio::sync::{Notify, mpsc, oneshot};

use super::priority::{LANE_COUNT, Priority, SchedulerState, pick_lane};

/// Intake channel capacity per lane. The walk backpressures on `send` once
/// this many scan files are queued ahead of the workers; the priority lanes
/// stay far below it in practice.
pub const INTAKE_CAPACITY: usize = 1024;

/// What a queued item asks the worker to do.
pub enum IntakeKind {
    /// Walk-driven scan file, counted against the scan job's barrier.
    Scan {
        /// Scan job this item belongs to.
        job_id: u64,
    },
    /// Watcher-debounced create/modify: index the file, report errors
    /// through the error handler (no job barrier).
    WatcherIndex,
    /// Watcher-observed removal: detach the path, then flush + update
    /// barriers (no job barrier).
    WatcherRemove,
    /// Periodic expiry sweep (no path, no job barrier).
    ExpireCheck,
    /// Request-path album self-update. `reply` carries the outcome back to
    /// the request; `None` means fire-and-forget (errors are logged).
    InteractiveAlbum {
        album_id: ArrayString<64>,
        reply: Option<oneshot::Sender<Result<()>>>,
    },
}

/// One queued background item. The priority lane travels on every item.
pub struct IntakeItem {
    /// Absolute file path (meaningful for path kinds only).
    pub path: PathBuf,
    /// Priority lane this item was submitted on.
    pub priority: Priority,
    /// What the worker should do with the item.
    pub kind: IntakeKind,
    /// Per-path generation at enqueue; a newer submit supersedes this item.
    pub generation: u64,
    /// File mtime observed at enqueue.
    pub mtime: Option<SystemTime>,
    /// File length observed at enqueue.
    pub len: Option<u64>,
}

/// One lane's bounded channel. The receiver sits behind a plain mutex: dequeue
/// only ever `try_recv`s, so the critical section is short and never held
/// across an await.
struct LaneQueue {
    tx: mpsc::Sender<IntakeItem>,
    rx: Mutex<mpsc::Receiver<IntakeItem>>,
}

impl LaneQueue {
    fn channel() -> Self {
        let (tx, rx) = mpsc::channel(INTAKE_CAPACITY);
        Self {
            tx,
            rx: Mutex::new(rx),
        }
    }
}

static LANES: LazyLock<[LaneQueue; LANE_COUNT]> = LazyLock::new(|| {
    [
        LaneQueue::channel(),
        LaneQueue::channel(),
        LaneQueue::channel(),
        LaneQueue::channel(),
        LaneQueue::channel(),
    ]
});

/// Wakes one parked worker per submit. The permit is stored, so a submit
/// that lands between the last `try_recv` and the park still wakes the
/// waiter.
static INTAKE_NOTIFY: Notify = Notify::const_new();

/// Round-robin scheduler state shared by the workers.
static SCHED: LazyLock<Mutex<SchedulerState>> =
    LazyLock::new(|| Mutex::new(SchedulerState::default()));

/// Latest submitted generation per path. Supersedes the `IN_PROGRESS` set:
/// instead of refusing a duplicate submit, a newer generation lets the older
/// queued item be skipped at dequeue time.
static GENERATIONS: LazyLock<DashMap<PathBuf, u64>> = LazyLock::new(DashMap::new);

/// Per-job submit/complete accounting backing [`super::worker::drain_intake`].
pub(crate) struct JobProgress {
    submitted: AtomicU64,
    completed: AtomicU64,
    notify: Notify,
}

impl JobProgress {
    /// `(submitted, completed)` snapshot for the drain barrier.
    pub(crate) fn counts(&self) -> (u64, u64) {
        (
            self.submitted.load(Ordering::SeqCst),
            self.completed.load(Ordering::SeqCst),
        )
    }

    /// Wait for the next completion. Each completion stores a [`Notify`]
    /// permit, so no wakeup is lost between [`JobProgress::counts`] and
    /// this wait.
    pub(crate) async fn wait_for_completion(&self) {
        self.notify.notified().await;
    }
}

pub(crate) fn remove_job(job_id: u64) {
    JOB_PROGRESS.remove(&job_id);
}

static JOB_PROGRESS: LazyLock<DashMap<u64, Arc<JobProgress>>> = LazyLock::new(DashMap::new);

pub(crate) fn job_progress(job_id: u64) -> Arc<JobProgress> {
    JOB_PROGRESS
        .entry(job_id)
        .or_insert_with(|| {
            Arc::new(JobProgress {
                submitted: AtomicU64::new(0),
                completed: AtomicU64::new(0),
                notify: Notify::new(),
            })
        })
        .clone()
}

pub(crate) fn note_item_complete(progress: &JobProgress) {
    progress.completed.fetch_add(1, Ordering::SeqCst);
    progress.notify.notify_one();
}

/// Drop the generation record for `item` when no newer submit replaced it,
/// so the map does not grow across scans.
pub(crate) fn release_generation(item: &IntakeItem) {
    GENERATIONS.remove_if(&item.path, |_, generation| *generation == item.generation);
}

/// Latest submitted generation for `path`, if any.
pub(crate) fn current_generation(path: &Path) -> Option<u64> {
    GENERATIONS.get(path).map(|entry| *entry)
}

/// Next submission generation for `path`.
fn next_generation(path: &Path) -> u64 {
    let mut slot = GENERATIONS.entry(path.to_path_buf()).or_insert(0);
    *slot += 1;
    *slot
}

static WORKERS_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

/// Start the fixed worker pool once. Workers run on the shared runtime via
/// [`crate::tasks::runtime::spawn`]; one in-flight file per core bounds the
/// fan-out the walk used to leave unbounded.
fn ensure_workers() {
    WORKERS_STARTED.get_or_init(|| {
        for _ in 0..*crate::tasks::runtime::CURRENT_NUM_THREADS {
            crate::tasks::runtime::spawn(super::worker::worker_loop());
        }
    });
}

/// Enqueue `item` on its lane, backpressuring the submitter past
/// [`INTAKE_CAPACITY`], and wake one parked worker.
async fn enqueue(item: IntakeItem) -> Result<()> {
    ensure_workers();
    LANES[item.priority.lane_index()]
        .tx
        .send(item)
        .await
        .context("scan intake closed")?;
    INTAKE_NOTIFY.notify_one();
    Ok(())
}

/// Observe the file identity at enqueue for staleness checks.
fn observe_identity(path: &Path) -> (Option<SystemTime>, Option<u64>) {
    match std::fs::metadata(path) {
        Ok(meta) => (meta.modified().ok(), Some(meta.len())),
        Err(_) => (None, None),
    }
}

/// Enqueue `path` for background indexing under scan job `job_id`.
///
/// The send backpressures the walk once [`INTAKE_CAPACITY`] items are
/// queued. A closed channel can only mean the worker pool is gone, in
/// which case the item is marked complete so the drain barrier cannot stall
/// on it.
pub async fn submit_scan(path: PathBuf, job_id: u64) -> Result<()> {
    let (mtime, len) = observe_identity(&path);
    let generation = next_generation(&path);
    let progress = job_progress(job_id);
    progress.submitted.fetch_add(1, Ordering::SeqCst);
    let item = IntakeItem {
        path,
        priority: Priority::Scan,
        kind: IntakeKind::Scan { job_id },
        generation,
        mtime,
        len,
    };
    if let Err(err) = enqueue(item).await {
        note_item_complete(&progress);
        return Err(err);
    }
    Ok(())
}

/// Enqueue a watcher-debounced create/modify for background indexing.
/// Shares the path generation map with the scan lane, so a newer submit for
/// the same path supersedes an older queued item in either lane.
pub async fn submit_watcher_index(path: PathBuf) -> Result<()> {
    let (mtime, len) = observe_identity(&path);
    enqueue(IntakeItem {
        path: path.clone(),
        priority: Priority::Watcher,
        kind: IntakeKind::WatcherIndex,
        generation: next_generation(&path),
        mtime,
        len,
    })
    .await
}

/// Enqueue a watcher-observed removal. The file is gone by definition, so
/// only the generation travels; a concurrent re-create (newer generation)
/// supersedes the removal before it can detach a live record.
pub async fn submit_watcher_remove(path: PathBuf) -> Result<()> {
    let generation = next_generation(&path);
    enqueue(IntakeItem {
        path,
        priority: Priority::Watcher,
        kind: IntakeKind::WatcherRemove,
        generation,
        mtime: None,
        len: None,
    })
    .await
}

/// Enqueue a periodic expiry sweep on the Expire lane.
pub async fn submit_expire_check() -> Result<()> {
    enqueue(IntakeItem {
        path: PathBuf::new(),
        priority: Priority::Expire,
        kind: IntakeKind::ExpireCheck,
        generation: 0,
        mtime: None,
        len: None,
    })
    .await
}

/// Enqueue a request-path album self-update on the Interactive lane and wait
/// for the worker to run it, reporting the outcome back to the request.
pub async fn submit_interactive_album(album_id: ArrayString<64>) -> Result<()> {
    let (tx, rx) = oneshot::channel();
    enqueue(IntakeItem {
        path: PathBuf::new(),
        priority: Priority::Interactive,
        kind: IntakeKind::InteractiveAlbum {
            album_id,
            reply: Some(tx),
        },
        generation: 0,
        mtime: None,
        len: None,
    })
    .await?;
    rx.await
        .map_err(|_| anyhow::anyhow!("interactive worker dropped the album result"))?
}

/// Enqueue a fire-and-forget album self-update on the Interactive lane.
/// Worker errors are logged, never propagated.
pub async fn submit_interactive_album_detached(album_id: ArrayString<64>) -> Result<()> {
    enqueue(IntakeItem {
        path: PathBuf::new(),
        priority: Priority::Interactive,
        kind: IntakeKind::InteractiveAlbum {
            album_id,
            reply: None,
        },
        generation: 0,
        mtime: None,
        len: None,
    })
    .await
}

/// Take the next queued item in lane order. `None` means every lane closed,
/// which only happens at shutdown; workers then exit.
pub(crate) async fn recv_item() -> Option<IntakeItem> {
    loop {
        let pick = {
            let mut sched = SCHED.lock().expect("scheduler lock poisoned");
            let mut ready = [false; LANE_COUNT];
            for (index, lane) in LANES.iter().enumerate() {
                ready[index] = !lane.rx.lock().expect("lane lock poisoned").is_empty();
            }
            pick_lane(ready, &mut sched)
        };
        match pick {
            Some(index) => {
                let lane = &LANES[index];
                match lane.rx.lock().expect("lane lock poisoned").try_recv() {
                    // Another worker won the race; recompute instead of
                    // parking, so no wakeup is lost.
                    Ok(item) => return Some(item),
                    Err(mpsc::error::TryRecvError::Empty) => continue,
                    Err(mpsc::error::TryRecvError::Disconnected) => return None,
                }
            }
            None => {
                if LANES
                    .iter()
                    .all(|lane| lane.rx.lock().expect("lane lock poisoned").is_closed())
                {
                    return None;
                }
            }
        }
        INTAKE_NOTIFY.notified().await;
    }
}
