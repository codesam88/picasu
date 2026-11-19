use crate::storage::cache::{EXPIRE, EXPIRE_TABLE_DEFINITION};
use crate::storage::db::VERSION_COUNT_TIMESTAMP;
use crate::tasks::queue::writer::{WriteOp, submit};
use chrono::Utc;
use mini_executor::BatchTask;
use std::sync::atomic::Ordering;
use std::time::Duration;

pub struct UpdateExpireTask;

impl BatchTask for UpdateExpireTask {
    async fn batch_run(_: Vec<Self>) {
        if update_expire_task() {
            match submit(WriteOp::ExpireCheck).await {
                Ok(()) => {}
                Err(err) => log::warn!("update expire submit failed: {err:#}"),
            }
        }
    }
}

/// Refresh the expiry bookkeeping timestamp. Returns whether an expiry sweep
/// is due: the `BatchTask` path submits [`WriteOp::ExpireCheck`] to the
/// serial writer in that case, while the serial writer's own cascade path
/// runs its inline sweep instead.
pub(crate) fn update_expire_task() -> bool {
    let current_timestamp = Utc::now().timestamp_millis();
    let last_timestamp = VERSION_COUNT_TIMESTAMP.swap(current_timestamp, Ordering::SeqCst);

    if last_timestamp > 0 {
        let slot_guard = crate::tasks::txn_probe::slot_guard(
            crate::tasks::txn_probe::Db::Expire,
            "update_expire:30",
        );
        let expire_write_txn = EXPIRE
            .in_disk
            .begin_write()
            .expect("failed to begin write transaction");
        slot_guard.acquired();
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
