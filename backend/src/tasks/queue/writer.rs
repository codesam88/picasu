//! Serial background DB writer with generation barriers, plus its ordered
//! cascade stage.
//!
//! One tokio task owns every background `TREE` `in_disk` (and snapshot DB)
//! write transaction. Background producers — scan/watcher flushes, the sweep,
//! expiry — submit [`WriteOp`]s over a bounded mpsc channel instead of
//! spawning detached batches, so overlapping write batches can no longer
//! interleave on redb's single-writer transactions and backpressure holds end
//! to end (a full channel parks the submitter).
//!
//! # Two stages: writer, then cascade
//!
//! The writer task performs ONLY redb write transactions: `Flush`
//! inserts/removes, `SweepDirAlbums` removes, `PruneUsersRoot` removes, and
//! `Barrier` markers (which carry no work). It never awaits the CPU
//! admittance semaphore, never touches rayon, and never runs cascades, so no
//! single op can head-of-line-block the writer on compute: every writer op
//! completes in well under a second.
//!
//! Follow-up work — pending album self-updates, update-expire bookkeeping,
//! and the expiry sweep — moves to ordered cascade jobs on a second bounded
//! channel, consumed serially by the cascade task (spawned alongside the
//! writer, on the shared runtime). Cascade handlers reuse the
//! [`run_cpu`](super::bridge::run_cpu) bridge, so compute still holds one
//! admittance permit per job against the N-1 pool. Each writer op maps to
//! exactly one cascade entry carrying the same completion seq:
//!
//! * `Flush` → tables are written and the asset-view epoch advances, then
//!   [`CascadeOp::AfterWrite`] runs pending album self-updates, update-expire
//!   bookkeeping, and the expire sweep when one is due.
//! * `ExpireCheck` → no writer-side work, then [`CascadeOp::ExpireRound`]
//!   (expiry sweep on the Rayon pool, then the looper timer re-arm — the
//!   pairing the old `ExpireCheckTask::batch_run` provided).
//! * `SweepDirAlbums` / `PruneUsersRoot` → removes are written and the epoch
//!   advances, then [`CascadeOp::Noop`].
//! * `Barrier` → marker only, then `Noop`.
//!
//! The asset view is never rebuilt here. `snapshot_tree` rebuilds it on
//! demand when the epoch moved, so a scan that writes thousands of rows costs
//! one rebuild per reader, not one per flush.
//!
//! # Barrier coverage across both stages
//!
//! The generation seq is one clock shared by both stages; each stage tracks
//! its own completion watermark over it. The writer records the highest fully
//! written seq in `COMPLETED_SEQ`; the cascade task records the highest fully
//! cascaded seq in `CASCADE_COMPLETED_SEQ`. Both advance through the same
//! [`BARRIER_NOTIFY`].
//!
//! Coverage is exact because both queues are FIFO and every writer op produces
//! exactly one cascade entry, enqueued in writer-completion order *before* the
//! writer advances its watermark:
//!
//! * A merged leading-flush run completes once at the run's max seq; the run
//!   sits ahead of everything behind it in the inbox, so every member seq —
//!   and the single shared cascade entry — precedes any later barrier marker
//!   in queue order.
//! * A barrier marker flows through both queues in submit order. Waiting until
//!   `COMPLETED_SEQ >= seq` proves every op ahead of the marker is written;
//!   waiting until `CASCADE_COMPLETED_SEQ >= seq` proves every cascade entry
//!   ahead of the marker — one per such op — has finished.
//!
//! FIFO order plus the 1:1 op-to-cascade mapping is what makes
//! `wait-for-<=seq` exact: when both watermarks pass the barrier marker,
//! every op with a smaller seq — and everything those ops cascaded — has
//! finished.
//!
//! Barrier waits carry NO timeout by design: a barrier that can return
//! early is not a barrier. If the writer ever truly stalls, the scan (or
//! shutdown drain) hangs loudly instead of proceeding with unflushed
//! state — a stuck scan is observable and debuggable, silent state drift
//! is not. (A fixed 120 s deadline was tried and produced false alarms
//! under small core budgets, where a deep but healthy writer queue
//! legitimately exceeds it.)
//!
//! # Scope
//!
//! Background paths only. Request-path routers keep their direct writes and
//! `BATCH_COORDINATOR` batches (see the deferred request-path migration), so
//! the writer does not — and cannot yet — own literally every `TREE` write
//! transaction. Snapshot flushes triggered by requests (`get_prefetch`) also
//! stay on the old path; the snapshot-DB writes that belong to background
//! work (expiry deletes, tree-snapshot removes) run inside the cascade
//! stage's `ExpireRound`.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use arrayvec::ArrayString;
use futures::FutureExt;
use tokio::sync::{Mutex, Notify, mpsc};

use super::bridge::run_cpu;
use crate::model::abstract_data::AbstractData;
use crate::process::dir_album::drain_pending_album_updates;
use crate::storage::tree_view::bump_tree_epoch;
use crate::tasks::actor::album::album_task;
use crate::tasks::batcher::expire_check::expire_check_task;
use crate::tasks::batcher::flush_tree::flush_tables;
use crate::tasks::batcher::update_expire::update_expire_task;
use crate::tasks::runtime::spawn;

/// Bound for the writer inbox. A full inbox parks the submitter, which is the
/// backpressure that replaces the old unbounded detached-batch channels.
const WRITER_CAPACITY: usize = 1024;

/// Bound for the cascade inbox. A full inbox parks the writer (which in turn
/// backpressures submitters through [`WRITER_CAPACITY`]), so end-to-end
/// backpressure holds across both stages.
const CASCADE_CAPACITY: usize = 1024;

/// One background write request. `Barrier` is a marker only: completing it
/// proves every earlier-submitted op (plus its cascade) finished.
pub enum WriteOp {
    /// Asset-table insert/remove batch (scan/watcher/sweep flushes).
    Flush {
        inserts: Vec<AbstractData>,
        removes: Vec<AbstractData>,
    },
    /// Expiry sweep over the snapshot caches.
    ExpireCheck,
    /// Sweep removal of dir-album records whose directories are gone.
    SweepDirAlbums(Vec<(PathBuf, ArrayString<64>)>),
    /// Removal of the album record on the `users_home` root itself.
    PruneUsersRoot(PathBuf),
    /// Batched sweep removal of asset records by exact path/id only.
    SweepRemoveBatch { paths: Vec<PathBuf> },
    /// Barrier marker; carries no work.
    Barrier,
}

/// Follow-up work for one completed writer op, in writer-completion order.
/// Each entry carries the completion seq of the writer op it follows, so the
/// cascade watermark advances over the same clock the writer advances.
enum CascadeOp {
    /// Pending album self-updates plus expiry bookkeeping. The asset view is
    /// not rebuilt here: writers only advance its epoch.
    AfterWrite,
    /// One expiry sweep plus the looper timer re-arm.
    ExpireRound,
    /// No follow-up work (sweep/prune removes, barrier markers).
    Noop,
}

/// Next generation seq to hand out. Monotonic across the process lifetime;
/// test resets never need to touch it because barriers only compare order.
static NEXT_SEQ: AtomicU64 = AtomicU64::new(1);

/// Highest seq fully written by the writer task.
static COMPLETED_SEQ: AtomicU64 = AtomicU64::new(0);

/// Highest seq whose cascade entry has fully completed.
static CASCADE_COMPLETED_SEQ: AtomicU64 = AtomicU64::new(0);

/// Woken after every watermark advance on either stage.
static BARRIER_NOTIFY: Notify = Notify::const_new();

/// Serializes sequence allocation with the channel send. The barrier proof
/// depends on channel arrival order matching sequence order; allocating
/// before awaiting `send` would let a later op overtake an earlier one.
static SUBMIT_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Set by [`shutdown_writer`] after the final barrier. Submissions are
/// rejected from then on, so the drain cannot be reopened by a late producer.
static WRITER_CLOSED: AtomicBool = AtomicBool::new(false);

/// Test-only hook: when set, the next cascade entry panics. Used by the
/// regression test for panic containment in [`cascade_loop`].
#[cfg(test)]
static FORCE_CASCADE_PANIC: AtomicBool = AtomicBool::new(false);

/// Current generation watermarks `(next, writer, cascade)`. Diagnostics only;
/// the values are read individually and may not form one instant.
#[cfg(feature = "task-observer")]
pub(crate) fn watermarks() -> (u64, u64, u64) {
    (
        NEXT_SEQ.load(Ordering::Relaxed),
        COMPLETED_SEQ.load(Ordering::Relaxed),
        CASCADE_COMPLETED_SEQ.load(Ordering::Relaxed),
    )
}

struct WriterEndpoint {
    tx: mpsc::Sender<(u64, WriteOp)>,
    handle: tokio::task::JoinHandle<()>,
}

static WRITER: LazyLock<Mutex<Option<WriterEndpoint>>> = LazyLock::new(|| Mutex::new(None));

struct CascadeEndpoint {
    tx: mpsc::Sender<(u64, CascadeOp)>,
    handle: tokio::task::JoinHandle<()>,
}

static CASCADE: LazyLock<Mutex<Option<CascadeEndpoint>>> = LazyLock::new(|| Mutex::new(None));

/// Start the cascade task once; later calls reuse the running task.
async fn ensure_cascade() -> mpsc::Sender<(u64, CascadeOp)> {
    let mut slot = CASCADE.lock().await;
    if let Some(endpoint) = slot.as_ref() {
        return endpoint.tx.clone();
    }
    let (tx, rx) = mpsc::channel(CASCADE_CAPACITY);
    let handle = spawn(cascade_loop(rx));
    let endpoint = CascadeEndpoint {
        tx: tx.clone(),
        handle,
    };
    *slot = Some(endpoint);
    tx
}

/// Start the writer task once; later calls reuse the running writer. The
/// cascade task starts first so the writer never enqueues into nothing.
async fn ensure_writer() -> anyhow::Result<mpsc::Sender<(u64, WriteOp)>> {
    if WRITER_CLOSED.load(Ordering::SeqCst) {
        return Err(anyhow::anyhow!("serial db writer is closed"));
    }
    let cascade_tx = ensure_cascade().await;
    let mut slot = WRITER.lock().await;
    if WRITER_CLOSED.load(Ordering::SeqCst) {
        return Err(anyhow::anyhow!("serial db writer is closed"));
    }
    if let Some(endpoint) = slot.as_ref() {
        return Ok(endpoint.tx.clone());
    }
    let (tx, rx) = mpsc::channel(WRITER_CAPACITY);
    let handle = spawn(writer_loop(rx, cascade_tx));
    let endpoint = WriterEndpoint {
        tx: tx.clone(),
        handle,
    };
    *slot = Some(endpoint);
    Ok(tx)
}

/// Allocate a sequence and send under one lock, so arrival order matches
/// sequence order. Returns the op's own seq, which is what a barrier waits
/// for.
async fn submit_tracked(op: WriteOp) -> anyhow::Result<u64> {
    let _guard = SUBMIT_LOCK.lock().await;
    if WRITER_CLOSED.load(Ordering::SeqCst) {
        return Err(anyhow::anyhow!("serial db writer is closed"));
    }
    let tx = ensure_writer().await?;
    let seq = NEXT_SEQ.fetch_add(1, Ordering::SeqCst);
    tx.send((seq, op))
        .await
        .map_err(|_| anyhow::anyhow!("serial db writer is gone"))?;
    Ok(seq)
}

/// Submit one op, backpressuring the caller while the inbox is full.
pub async fn submit(op: WriteOp) -> anyhow::Result<()> {
    submit_tracked(op).await.map(|_| ())
}

/// Submit a flush of scanned/mutated assets.
pub async fn submit_flush_insert(inserts: Vec<AbstractData>) -> anyhow::Result<()> {
    submit(WriteOp::Flush {
        inserts,
        removes: Vec::new(),
    })
    .await
}

/// Submit a flush of pruned asset records (sweep, watcher removals).
pub async fn submit_flush_remove(removes: Vec<AbstractData>) -> anyhow::Result<()> {
    submit(WriteOp::Flush {
        inserts: Vec::new(),
        removes,
    })
    .await
}

/// Wait until every op submitted before this call — including everything
/// those ops cascaded through the follow-up stage — has completed. No
/// timeout: see the module docs for why a barrier must not return early.
pub async fn barrier() {
    if let Ok(seq) = submit_tracked(WriteOp::Barrier).await {
        wait_for_seq(seq).await;
    }
}

/// Drain both stages at shutdown: wait until everything submitted so far —
/// writes and cascades — has completed, then close both inboxes and join
/// both tasks. The cascade channel drains its buffer before its loop exits,
/// so no enqueued cascade is lost.
pub async fn shutdown_writer() {
    barrier().await;
    let guard = SUBMIT_LOCK.lock().await;
    WRITER_CLOSED.store(true, Ordering::SeqCst);
    let endpoint = WRITER.lock().await.take();
    drop(guard);
    if let Some(endpoint) = endpoint {
        drop(endpoint.tx);
        if let Err(err) = endpoint.handle.await {
            log::error!("serial db writer task panicked: {err}");
        }
    }
    let cascade = CASCADE.lock().await.take();
    if let Some(cascade) = cascade {
        drop(cascade.tx);
        if let Err(err) = cascade.handle.await {
            log::error!("writer cascade task panicked: {err}");
        }
    }
}

async fn wait_for_seq(seq: u64) {
    // `BARRIER_NOTIFY` wakes via `notify_waiters`, which stores no permit:
    // a notification that fires between the watermark check and the wait
    // would otherwise be lost forever once no more ops complete. `enable()`
    // registers this waiter *before* the check, closing that race — the
    // check-then-wait order below is exact, not optimistic.
    loop {
        let notified = BARRIER_NOTIFY.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if COMPLETED_SEQ.load(Ordering::SeqCst) >= seq
            && CASCADE_COMPLETED_SEQ.load(Ordering::SeqCst) >= seq
        {
            return;
        }
        notified.await;
    }
}

/// Writer main loop: strictly FIFO, redb write transactions only. Leading
/// `Flush` runs merge into one batch (the old `BatchTask` coalescing); every
/// op maps to exactly one cascade entry enqueued in completion order before
/// the writer watermark advances, so barriers stay exact across both stages.
async fn writer_loop(
    mut rx: mpsc::Receiver<(u64, WriteOp)>,
    cascade_tx: mpsc::Sender<(u64, CascadeOp)>,
) {
    let mut pending: VecDeque<(u64, WriteOp)> = VecDeque::new();
    loop {
        if pending.is_empty() {
            let Some(msg) = rx.recv().await else {
                return;
            };
            pending.push_back(msg);
        }
        while let Ok(msg) = rx.try_recv() {
            pending.push_back(msg);
        }
        // Merge the leading run of flushes into one batch plus one cascade.
        let mut inserts = Vec::new();
        let mut removes = Vec::new();
        let mut flush_seq = 0u64;
        let mut merged = 0usize;
        while matches!(pending.front(), Some((_, WriteOp::Flush { .. }))) {
            let (seq, op) = pending.pop_front().expect("flush run checked above");
            let WriteOp::Flush {
                inserts: mut ins,
                removes: mut rem,
            } = op
            else {
                unreachable!("flush run checked above");
            };
            inserts.append(&mut ins);
            removes.append(&mut rem);
            flush_seq = flush_seq.max(seq);
            merged += 1;
        }
        if merged > 0 {
            flush_tables(&inserts, &removes);
            bump_tree_epoch();
            enqueue_cascade(&cascade_tx, flush_seq, CascadeOp::AfterWrite).await;
            complete_seq(flush_seq);
        }
        if let Some((seq, op)) = pending.pop_front() {
            let cascade = handle_op(op);
            enqueue_cascade(&cascade_tx, seq, cascade).await;
            complete_seq(seq);
        }
    }
}

fn complete_seq(seq: u64) {
    COMPLETED_SEQ.fetch_max(seq, Ordering::SeqCst);
    BARRIER_NOTIFY.notify_waiters();
}

async fn enqueue_cascade(cascade_tx: &mpsc::Sender<(u64, CascadeOp)>, seq: u64, op: CascadeOp) {
    if cascade_tx.send((seq, op)).await.is_err() {
        // The cascade task is gone: barriers below this seq hang loudly
        // rather than covering un-run cascades (see module docs).
        log::error!("writer cascade stage is gone; barriers will stall");
    }
}

/// Run one op's write transaction and return its follow-up cascade. Pure
/// redb writes — no awaits, no CPU semaphore, no rayon — so the writer task
/// stays under a second per op.
fn handle_op(op: WriteOp) -> CascadeOp {
    match op {
        WriteOp::Flush { inserts, removes } => {
            flush_tables(&inserts, &removes);
            bump_tree_epoch();
            CascadeOp::AfterWrite
        }
        WriteOp::ExpireCheck => CascadeOp::ExpireRound,
        WriteOp::SweepDirAlbums(entries) => {
            handle_sweep_dir_albums(entries);
            bump_tree_epoch();
            CascadeOp::Noop
        }
        WriteOp::PruneUsersRoot(users_root) => {
            handle_prune_users_root(&users_root);
            bump_tree_epoch();
            CascadeOp::Noop
        }
        WriteOp::SweepRemoveBatch { paths } => {
            crate::tasks::batcher::flush_tree::sweep_remove_batch(&paths);
            bump_tree_epoch();
            CascadeOp::Noop
        }
        WriteOp::Barrier => CascadeOp::Noop,
    }
}

/// Cascade main loop: strictly FIFO, one entry per completed writer op.
/// Serial consumption preserves the exact flush→rebuild→expire order per seq.
async fn cascade_loop(mut rx: mpsc::Receiver<(u64, CascadeOp)>) {
    loop {
        let Some((seq, op)) = rx.recv().await else {
            return;
        };
        // Contain panics per entry: a panicking cascade (e.g. an inline
        // `.expect` in `update_expire_task`) must not kill the loop or freeze
        // `CASCADE_COMPLETED_SEQ`, otherwise barriers stall forever. Log and
        // skip, then advance the watermark for this entry.
        let result = std::panic::AssertUnwindSafe(handle_cascade(op))
            .catch_unwind()
            .await;
        if let Err(panic) = result {
            let message = panic
                .downcast_ref::<&'static str>()
                .copied()
                .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("unknown panic");
            log::error!("writer cascade entry panicked: {message}");
        }
        CASCADE_COMPLETED_SEQ.fetch_max(seq, Ordering::SeqCst);
        BARRIER_NOTIFY.notify_waiters();
    }
}

async fn handle_cascade(op: CascadeOp) {
    #[cfg(test)]
    assert!(
        !FORCE_CASCADE_PANIC.swap(false, Ordering::SeqCst),
        "injected cascade panic (test)"
    );
    match op {
        CascadeOp::AfterWrite => cascade_after_write().await,
        CascadeOp::ExpireRound => cascade_expire_round().await,
        CascadeOp::Noop => {}
    }
}

/// Run pending album self-updates serially, then expiry bookkeeping.
/// Asset rows are already durable; the in-memory view refreshes lazily from
/// its epoch, so this stage never rebuilds it.
async fn cascade_after_write() {
    let pending = drain_pending_album_updates();
    // Snapshot the tree once per cascade entry; album tasks share it through
    // an `Arc` and never rebuild (or touch snapshot_tree) themselves.
    let ref_data: std::sync::Arc<[crate::model::response::DatabaseTimestamp]> =
        crate::storage::db::snapshot_tree().into();
    for album_id in pending {
        let ref_data = std::sync::Arc::clone(&ref_data);
        if let Err(err) = run_cpu(move || album_task(album_id, &ref_data)).await {
            log::warn!("cascade album self-update failed for {album_id}: {err:#}");
        }
    }
    cascade_update_expire().await;
}

/// Expiry bookkeeping plus the sweep, ordered behind the tree rebuild.
async fn cascade_update_expire() {
    if update_expire_task() {
        cascade_expire_round().await;
    }
}

/// One expiry sweep on the Rayon pool (its inner `par_*` sections nest on
/// the same pool), then re-arm the looper timer — the pairing the old
/// `ExpireCheckTask::batch_run` provided.
async fn cascade_expire_round() {
    if let Err(err) = run_cpu(|| {
        expire_check_task();
        Ok::<(), anyhow::Error>(())
    })
    .await
    {
        log::warn!("cascade expire check failed: {err:#}");
    }
    crate::tasks::looper::reset_expire_check_timer().await;
}

/// Sweep removal of dir-album records whose directories no longer exist.
/// Asset-table and metadata writes run here, inside the writer; the caller
/// evicts nothing — cache eviction is part of this op so DB and cache stay
/// ordered.
fn handle_sweep_dir_albums(entries: Vec<(PathBuf, ArrayString<64>)>) {
    use crate::storage::db::{METADATA_TABLE, TREE};

    for (dir_path, album_id) in entries {
        let _ = crate::storage::asset_store::remove_asset_by_id(album_id.as_str());
        let _ =
            crate::storage::asset_store::remove_asset_by_path(dir_path.to_string_lossy().as_ref());
        if let Ok(txn) = TREE.in_disk.begin_write() {
            if let Ok(mut table) = txn.open_table(METADATA_TABLE) {
                let _ = table.remove(&*album_id);
            }
            let _ = txn.commit();
        }
        crate::process::dir_album::evict_dir_album(&dir_path);
    }
}

/// Removal of the album record on the `users_home` root itself, which must
/// never carry one.
fn handle_prune_users_root(users_root: &std::path::Path) {
    use crate::storage::db::{METADATA_TABLE, TREE};

    let users_root_str = users_root.to_string_lossy().to_string();
    let Ok(Some(album_id)) = crate::storage::asset_store::get_asset_id_by_path(&users_root_str)
    else {
        return;
    };
    let _ = crate::storage::asset_store::remove_asset_by_id(album_id.as_str());
    let _ = crate::storage::asset_store::remove_asset_by_path(&users_root_str);
    if let Ok(txn) = TREE.in_disk.begin_write() {
        if let Ok(mut table) = txn.open_table(METADATA_TABLE) {
            let _ = table.remove(&*album_id);
        }
        let _ = txn.commit();
    }
    crate::process::dir_album::evict_dir_album(users_root);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn cascade_two_pending_albums_rebuilds_at_most_once() {
        let _ = &*crate::tests::bootstrap::TEST_ENV;
        crate::tasks::observe::reset();
        crate::process::dir_album::mark_album_for_update(
            arrayvec::ArrayString::from("album-a").expect("fits"),
        );
        crate::process::dir_album::mark_album_for_update(
            arrayvec::ArrayString::from("album-b").expect("fits"),
        );
        cascade_after_write().await;
        let recorded = crate::tasks::observe::snapshot()
            .stage(crate::tasks::observe::Stage::TreeRefreshBuild)
            .recorded;
        assert!(
            recorded <= 1,
            "expected at most one TreeRefreshBuild, got {recorded}"
        );
    }

    #[test]
    fn completed_watermarks_never_regress() {
        complete_seq(1_000);
        complete_seq(999);
        assert!(
            COMPLETED_SEQ.load(Ordering::SeqCst) >= 1_000,
            "writer watermark must not move backwards when an earlier seq completes late"
        );

        CASCADE_COMPLETED_SEQ.fetch_max(1_000, Ordering::SeqCst);
        CASCADE_COMPLETED_SEQ.fetch_max(999, Ordering::SeqCst);
        assert!(
            CASCADE_COMPLETED_SEQ.load(Ordering::SeqCst) >= 1_000,
            "cascade watermark must not move backwards"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn panicking_cascade_entry_does_not_stall_barrier() {
        let _ = &*crate::tests::bootstrap::TEST_ENV;
        crate::tasks::observe::reset();
        // Drain any prior entries so the injected panic lands on our flush.
        barrier().await;

        FORCE_CASCADE_PANIC.store(true, Ordering::SeqCst);

        let _ = submit_flush_insert(Vec::new()).await;

        let res = tokio::time::timeout(std::time::Duration::from_secs(5), barrier()).await;
        FORCE_CASCADE_PANIC.store(false, Ordering::SeqCst);

        assert!(
            res.is_ok(),
            "barrier stalled after a panicking cascade entry"
        );
    }
}
