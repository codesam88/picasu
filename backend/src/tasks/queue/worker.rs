//! Intake workers and the scan-done barrier.
//!
//! Workers dequeue [`IntakeItem`]s in lane order and run each through the
//! matching pipeline: scan and watcher files through
//! `workflow::process_file`, removals through the watcher removal handler,
//! expiry sweeps through `expire_check_task`, and Interactive album updates
//! through `album_task` with the outcome reported on the item's oneshot.
//! Completion feeds the per-job barrier in [`drain_intake`]; the `barrier_*`
//! shims reuse the existing `execute_batch_waiting` rendezvous at the same
//! call sites (same semantics, new names).

use super::bridge::{run_cpu, run_cpu_fifo, run_io};
use super::intake::{
    IntakeItem, IntakeKind, current_generation, job_progress, note_item_complete, recv_item,
    release_generation, remove_job,
};
use log::{debug, warn};

/// Worker main loop: dequeue intake items until every lane closes.
pub(crate) async fn worker_loop() {
    loop {
        let Some(item) = recv_item().await else {
            return;
        };
        dispatch_item(item).await;
    }
}

/// Route one item to its pipeline.
async fn dispatch_item(item: IntakeItem) {
    debug!(
        "intake worker: {} (lane {:?}, gen {})",
        item.path.display(),
        item.priority,
        item.generation
    );
    match item.kind {
        IntakeKind::Scan { job_id } => process_scan_item(item, job_id).await,
        IntakeKind::WatcherIndex => process_watcher_index(item).await,
        IntakeKind::WatcherRemove => process_watcher_remove(item).await,
        IntakeKind::ExpireCheck => process_expire_check().await,
        IntakeKind::InteractiveAlbum { album_id, reply } => {
            let outcome =
                run_cpu_fifo(move || crate::tasks::actor::album::album_task(album_id)).await;
            match reply {
                Some(tx) => {
                    let _ = tx.send(outcome);
                }
                None => {
                    if let Err(err) = outcome {
                        warn!("detached album self-update failed: {err:#}");
                    }
                }
            }
        }
    }
}

/// File identity captured at enqueue, for the staleness check.
fn enqueued_identity(item: &IntakeItem) -> (Option<std::time::SystemTime>, Option<u64>) {
    (item.mtime, item.len)
}

/// A newer submit for the same path supersedes this item: skip it without
/// touching the scan counters.
fn is_superseded(item: &IntakeItem) -> bool {
    current_generation(&item.path) != Some(item.generation)
}

/// The file changed since enqueue: skip it; a later scan picks it up. A
/// file that no longer stats is still processed so the open failure is
/// reported as a failed file, as before.
fn is_stale(item: &IntakeItem) -> bool {
    if let Ok(meta) = std::fs::metadata(&item.path) {
        let fresh = (meta.modified().ok(), Some(meta.len()));
        if fresh != enqueued_identity(item) {
            return true;
        }
    }
    false
}

/// Run one scan item and complete its share of the job barrier.
async fn process_scan_item(item: IntakeItem, job_id: u64) {
    let progress = job_progress(job_id);
    if is_superseded(&item) {
        note_item_complete(&progress);
        return;
    }
    if is_stale(&item) {
        release_generation(&item);
        note_item_complete(&progress);
        return;
    }

    match crate::workflow::process_file(item.path.clone()).await {
        Ok(()) => crate::tasks::actor::album_index::note_file_processed(job_id),
        Err(err) => {
            crate::error::handle_error(err);
            crate::tasks::actor::album_index::note_file_failed(
                job_id,
                Some(item.path.display().to_string()),
            );
        }
    }
    release_generation(&item);
    note_item_complete(&progress);
}

/// Run one watcher-debounced index item. Same pipeline as the scan lane,
/// without job-barrier accounting.
async fn process_watcher_index(item: IntakeItem) {
    if is_superseded(&item) {
        return;
    }
    if is_stale(&item) {
        release_generation(&item);
        return;
    }
    if let Err(err) = crate::workflow::process_file(item.path.clone()).await {
        crate::error::handle_error(err);
    }
    release_generation(&item);
}

/// Run one watcher-observed removal: detach the path on the blocking pool,
/// then the same flush + update barriers the removal used to run inline.
async fn process_watcher_remove(item: IntakeItem) {
    if is_superseded(&item) {
        return;
    }
    let path = item.path.clone();
    if let Err(err) = run_io(move || {
        crate::tasks::batcher::start_watcher::handle_removed_file(&path);
        Ok::<(), anyhow::Error>(())
    })
    .await
    {
        warn!(
            "removal handler failed for {}: {err:#}",
            item.path.display()
        );
    }
    release_generation(&item);
    barrier_flush_tree().await;
    barrier_update_tree().await;
}

/// Run one expiry sweep on the Rayon pool (its inner `par_*` sections nest
/// on the same pool) and re-arm the looper timer.
async fn process_expire_check() {
    if let Err(err) = run_cpu(|| {
        crate::tasks::batcher::expire_check::expire_check_task();
        Ok::<(), anyhow::Error>(())
    })
    .await
    {
        warn!("expire check failed: {err:#}");
    }
    crate::tasks::looper::reset_expire_check_timer().await;
}

/// Wait until every item submitted for `job_id` has completed. The walk has
/// finished submitting by the time this runs, so the submitted count is
/// final; each completion stores a [`Notify`](tokio::sync::Notify) permit,
/// so no wakeup is lost between the count check and the wait.
pub async fn drain_intake(job_id: u64) {
    let progress = job_progress(job_id);
    loop {
        let (submitted, completed) = progress.counts();
        if completed >= submitted {
            break;
        }
        progress.wait_for_completion().await;
    }
    remove_job(job_id);
}

/// Pre/post-sweep barrier: drains detached `FlushTreeTask` inserts so the
/// in-memory tree reflects all walk results before the sweep reads it.
/// Thin shim over the existing rendezvous (same call site, same semantics).
pub async fn barrier_flush_tree() {
    use crate::tasks::BATCH_COORDINATOR;
    use crate::tasks::batcher::flush_tree::FlushTreeTask;
    let _ = BATCH_COORDINATOR
        .execute_batch_waiting(FlushTreeTask::insert(vec![]))
        .await;
}

/// Post-sweep barrier: drains the sweep's detached remove/update batches,
/// then rebuilds album metadata. Thin shim over the existing rendezvous.
pub async fn barrier_update_tree() {
    use crate::tasks::BATCH_COORDINATOR;
    use crate::tasks::batcher::update_tree::UpdateTreeTask;
    let _ = BATCH_COORDINATOR
        .execute_batch_waiting(UpdateTreeTask)
        .await;
}
