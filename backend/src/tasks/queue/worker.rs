//! Intake workers and the scan-done barrier (slice 2a).
//!
//! Workers dequeue [`IntakeItem`]s and run each through
//! `workflow::process_file`, reporting the outcome on the same scan
//! counters the per-file tasks used. The drain barrier waits for every
//! submitted item of a job to complete, then the `barrier_*` shims reuse
//! the existing `execute_batch_waiting` rendezvous at the same call sites
//! (same semantics, new names).

use super::intake::{
    IntakeItem, current_generation, job_progress, note_item_complete, recv_item,
    release_generation, remove_job,
};
use log::debug;

/// Worker main loop: dequeue scan items until the intake closes.
pub(crate) async fn worker_loop() {
    loop {
        let Some(item) = recv_item().await else {
            return;
        };
        process_intake_item(item).await;
    }
}

/// File identity captured at enqueue, for the staleness check.
fn enqueued_identity(item: &IntakeItem) -> (Option<std::time::SystemTime>, Option<u64>) {
    (item.mtime, item.len)
}

/// Run one intake item and complete its share of the job barrier.
async fn process_intake_item(item: IntakeItem) {
    let progress = job_progress(item.job_id);
    debug!(
        "intake worker: {} (lane {:?}, gen {})",
        item.path.display(),
        item.priority,
        item.generation
    );

    // A newer submit for the same path supersedes this item: skip it.
    if current_generation(&item.path) != Some(item.generation) {
        note_item_complete(&progress);
        return;
    }

    // The file changed since enqueue: skip it; a later scan picks it up. A
    // file that no longer stats is still processed so the open failure is
    // reported as a failed file, as before.
    if let Ok(meta) = std::fs::metadata(&item.path) {
        let fresh = (meta.modified().ok(), Some(meta.len()));
        if fresh != enqueued_identity(&item) {
            release_generation(&item);
            note_item_complete(&progress);
            return;
        }
    }

    match crate::workflow::process_file(item.path.clone()).await {
        Ok(()) => crate::tasks::actor::album_index::note_file_processed(item.job_id),
        Err(err) => {
            crate::error::handle_error(err);
            crate::tasks::actor::album_index::note_file_failed(
                item.job_id,
                Some(item.path.display().to_string()),
            );
        }
    }
    release_generation(&item);
    note_item_complete(&progress);
}

/// Wait until every item submitted for `job_id` has completed. The walk has
/// finished submitting by the time this runs, so the submitted count is
/// final; each completion stores a [`Notify`] permit, so no wakeup is lost
/// between the count check and the wait.
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
