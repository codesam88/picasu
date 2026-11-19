use crate::model::response::Prefetch;
use crate::storage::cache::EXPIRE;
use crate::storage::cache::QUERY_SNAPSHOT;
use crate::storage::db::VERSION_COUNT_TIMESTAMP;

use crate::tasks::actor::remove_tree_snapshot::remove_task;
use redb::{ReadableDatabase, ReadableTable, TableDefinition, TableHandle};
use std::sync::atomic::Ordering;

use log::{debug, error};

pub(crate) fn expire_check_task() {
    // Read-collect phase: take a short read transaction, enumerate the query
    // snapshot tables, decide which ones are expired (which also prunes their
    // rows from the expire DB), and collect their tree-snapshot ids. The read
    // transaction is dropped before any write transaction opens, so a long
    // sweep can never hold a write slot while it scans. Tables created mid-sweep
    // simply are not in this snapshot and are picked up on the next round.
    let mut expired: Vec<(String, i64, Vec<i64>)> = Vec::new();
    {
        let read_txn = QUERY_SNAPSHOT
            .in_disk
            .begin_read()
            .expect("failed to begin read transaction");
        for table_handle in read_txn.list_tables().expect("failed to list tables") {
            let name = table_handle.name().to_string();
            if let Ok(timestamp) = name.parse::<i64>()
                && VERSION_COUNT_TIMESTAMP.load(Ordering::Relaxed) > timestamp
                && EXPIRE.expired_check(timestamp)
            {
                let table_definition: TableDefinition<u64, Prefetch> = TableDefinition::new(&name);
                let tree_ids: Vec<i64> = read_txn
                    .open_table(table_definition)
                    .ok()
                    .map(|table| {
                        table
                            .iter()
                            .ok()
                            .map(|iter| {
                                iter.filter_map(|result| {
                                    result.ok().map(|(_, guard)| guard.value().timestamp)
                                })
                                .collect()
                            })
                            .unwrap_or_default()
                    })
                    .unwrap_or_default();

                debug!("Collect expired query cache table: {timestamp}");
                expired.push((name, timestamp, tree_ids));
            }
        }
    }

    // Delete phase: one short write transaction per expired table, no rayon
    // bridge held across the write. Tree-snapshot removals run as separate
    // short transactions per id.
    for (name, timestamp, tree_ids) in expired {
        let slot_guard = crate::tasks::txn_probe::slot_guard(
            crate::tasks::txn_probe::Db::Cache,
            "expire_check:62",
        );
        let write_txn = QUERY_SNAPSHOT
            .in_disk
            .begin_write()
            .expect("failed to begin write transaction");
        slot_guard.acquired();

        let table_definition: TableDefinition<u64, Prefetch> = TableDefinition::new(&name);
        match write_txn.delete_table(table_definition) {
            Ok(true) => {
                debug!("Delete query cache table: {timestamp}");
            }
            Ok(false) => {
                error!("Failed to delete query cache table: {timestamp}");
            }
            Err(err) => {
                error!("Failed to delete query cache table: {timestamp}, error: {err:#?}");
            }
        }

        write_txn.commit().expect("failed to commit transaction");

        for tree_timestamp in tree_ids {
            remove_task(tree_timestamp);
        }
    }
}
