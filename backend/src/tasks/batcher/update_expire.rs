use crate::storage::cache::{EXPIRE, EXPIRE_TABLE_DEFINITION};
use crate::storage::db::VERSION_COUNT_TIMESTAMP;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::expire_check::ExpireCheckTask;
use chrono::Utc;
use mini_executor::BatchTask;
use std::sync::atomic::Ordering;
use std::time::Duration;

pub struct UpdateExpireTask;

impl BatchTask for UpdateExpireTask {
    fn batch_run(_: Vec<Self>) -> impl std::future::Future<Output = ()> + Send {
        if update_expire_task() {
            BATCH_COORDINATOR.execute_batch_detached(ExpireCheckTask);
        }
        std::future::ready(())
    }
}

/// Refresh the expiry bookkeeping timestamp. Returns whether an expiry sweep
/// is due: the `BatchTask` path cascades to [`ExpireCheckTask`] through
/// [`BATCH_COORDINATOR`] in that case, while the serial writer
/// (`tasks::queue::writer`) runs its own inline sweep instead.
pub(crate) fn update_expire_task() -> bool {
    let current_timestamp = Utc::now().timestamp_millis();
    let last_timestamp = VERSION_COUNT_TIMESTAMP.swap(current_timestamp, Ordering::SeqCst);

    if last_timestamp > 0 {
        let expire_write_txn = EXPIRE
            .in_disk
            .begin_write()
            .expect("failed to begin write transaction");
        let new_expire_time = current_timestamp.saturating_add(
            i64::try_from(Duration::from_secs(3600).as_millis())
                .expect("value out of range for i64"),
        );
        {
            let mut expire_table = expire_write_txn
                .open_table(EXPIRE_TABLE_DEFINITION)
                .expect("Failed to open expire table");

            expire_table
                .insert(last_timestamp, Some(new_expire_time))
                .expect("Failed to insert into expire table");
            expire_table
                .insert(current_timestamp, None)
                .expect("Failed to insert into expire table");

            debug!(
                "Expire table updated. Next expire time set to {}",
                new_expire_time
            );
        }

        expire_write_txn
            .commit()
            .expect("failed to commit transaction");
        true
    } else {
        false
    }
}
