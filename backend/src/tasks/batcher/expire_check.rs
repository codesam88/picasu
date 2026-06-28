use crate::model::response::Prefetch;
use crate::storage::cache::QUERY_SNAPSHOT;
use crate::storage::db::VERSION_COUNT_TIMESTAMP;
use crate::{storage::cache::EXPIRE, tasks::INDEX_COORDINATOR};

use crate::tasks::actor::remove_tree_snapshot::RemoveTask;
use crate::tasks::looper::reset_expire_check_timer;
use mini_executor::BatchTask;
use rayon::iter::{ParallelBridge, ParallelIterator};

use log::{error, info};
use redb::{ReadableDatabase, ReadableTable, TableDefinition, TableHandle};
use std::sync::atomic::Ordering;

pub struct ExpireCheckTask;

impl BatchTask for ExpireCheckTask {
    async fn batch_run(_: Vec<Self>) {
        expire_check_task();
        // Reset countdown timer after task execution
        reset_expire_check_timer().await;
    }
}

fn expire_check_task() {
    let write_txn = QUERY_SNAPSHOT
        .in_disk
        .begin_write()
        .expect("failed to begin write transaction");

    write_txn
        .list_tables()
        .expect("failed to list tables")
        .par_bridge()
        .for_each(|table_handle| {
            if let Ok(timestamp) = table_handle.name().parse::<i64>()
                && VERSION_COUNT_TIMESTAMP.load(Ordering::Relaxed) > timestamp
                && EXPIRE.expired_check(timestamp)
            {
                let binding = timestamp.to_string();
                let table_definition: TableDefinition<u64, Prefetch> =
                    TableDefinition::new(&binding);

                let read_txn = QUERY_SNAPSHOT
                    .in_disk
                    .begin_read()
                    .expect("failed to begin read transaction");
                let table = read_txn
                    .open_table(table_definition)
                    .expect("failed to open table");

                match write_txn.delete_table(table_handle) {
                    Ok(true) => {
                        info!("Delete query cache table: {timestamp}");
                        let tree_snapshot_delete_queue: Vec<_> = table
                            .iter()
                            .expect("failed to iterate table")
                            .par_bridge()
                            .map(|result| {
                                let (_, guard) = result.expect("failed to read record");
                                let prefetch_return = guard.value();
                                prefetch_return.timestamp
                            })
                            .collect();

                        for timestamp in tree_snapshot_delete_queue {
                            #[allow(clippy::let_underscore_future)]
                            let _ = INDEX_COORDINATOR.execute_detached(RemoveTask::new(timestamp));
                        }
                    }
                    Ok(false) => {
                        error!("Failed to delete query cache table: {timestamp}");
                    }
                    Err(err) => {
                        error!("Failed to delete query cache table: {timestamp}, error: {err:#?}");
                    }
                }

                info!(
                    "{} items remaining in disk query cache",
                    write_txn
                        .list_tables()
                        .expect("failed to list tables")
                        .count()
                );
            }
        });

    write_txn.commit().expect("failed to commit transaction");
}
