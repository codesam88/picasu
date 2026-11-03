//! Bounded intake channel with path coalescing and per-job progress.
//!
//! `submit_scan` records the file identity observed at enqueue
//! (`mtime` + `len`) and bumps the per-path generation. The worker skips an
//! item whose generation has been superseded by a newer submit, or whose
//! file changed since enqueue; both cases complete the per-job barrier
//! without touching the scan counters. A submit for a path that no longer
//! stats is still enqueued so `process_file` reports it as a failed file,
//! exactly as the per-file tasks did.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, OnceLock};

use anyhow::{Context, Result};
use dashmap::DashMap;
use tokio::sync::{Mutex, Notify, mpsc};

use super::priority::Priority;

/// Intake channel capacity. The walk backpressures on `send` once this many
/// files are queued ahead of the workers.
pub const INTAKE_CAPACITY: usize = 1024;

/// One queued scan item. The priority lane travels on every item even though
/// only [`Priority::Scan`] is submitted in this slice.
pub struct IntakeItem {
    /// Absolute file path to index.
    pub path: PathBuf,
    /// Priority lane (always `Scan` in this slice).
    pub priority: Priority,
    /// Scan job this item belongs to.
    pub job_id: u64,
    /// Per-path generation at enqueue; a newer submit supersedes this item.
    pub generation: u64,
    /// File mtime observed at enqueue.
    pub mtime: Option<std::time::SystemTime>,
    /// File length observed at enqueue.
    pub len: Option<u64>,
}

/// Latest submitted generation per path. Supersedes the `IN_PROGRESS` set:
/// instead of refusing a duplicate submit, a newer generation lets the older
/// queued item be skipped at dequeue time.
static GENERATIONS: LazyLock<DashMap<PathBuf, u64>> = LazyLock::new(DashMap::new);

/// Bounded intake channel. The receiver is shared by the worker pool behind
/// an async mutex; the sender side backpressures the walk past capacity.
static INTAKE: LazyLock<(mpsc::Sender<IntakeItem>, Mutex<mpsc::Receiver<IntakeItem>>)> =
    LazyLock::new(|| {
        let (tx, rx) = mpsc::channel(INTAKE_CAPACITY);
        (tx, Mutex::new(rx))
    });

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

static WORKERS_STARTED: OnceLock<()> = OnceLock::new();

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

/// Enqueue `path` for background indexing under `job_id`.
///
/// The send backpressures the walk once [`INTAKE_CAPACITY`] items are
/// queued. A closed channel can only mean the worker pool is gone, in
/// which case the item is marked complete so the drain barrier cannot stall
/// on it.
pub async fn submit_scan(path: PathBuf, job_id: u64) -> Result<()> {
    ensure_workers();
    let (mtime, len) = match std::fs::metadata(&path) {
        Ok(meta) => (meta.modified().ok(), Some(meta.len())),
        Err(_) => (None, None),
    };
    let generation = next_generation(&path);
    let progress = job_progress(job_id);
    progress.submitted.fetch_add(1, Ordering::SeqCst);
    let item = IntakeItem {
        path,
        priority: Priority::Scan,
        job_id,
        generation,
        mtime,
        len,
    };
    if let Err(err) = INTAKE.0.send(item).await {
        note_item_complete(&progress);
        return Err(err).context("scan intake closed");
    }
    Ok(())
}

/// Take the next queued item. `None` means the channel closed, which only
/// happens at shutdown; workers then exit.
pub(crate) async fn recv_item() -> Option<IntakeItem> {
    INTAKE.1.lock().await.recv().await
}
