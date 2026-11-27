use crate::storage::cache::QUERY_SNAPSHOT;
use crate::storage::db::VERSION_COUNT_TIMESTAMP;
use crate::storage::ser_de::QueryCacheEntry;

use mini_executor::BatchTask;
use redb::TableDefinition;
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::error::handle_error;
use anyhow;

pub struct FlushQuerySnapshotTask;

impl BatchTask for FlushQuerySnapshotTask {
    async fn batch_run(_: Vec<Self>) {
        flush_query_snapshot_task();
    }
}

pub(crate) fn flush_query_snapshot_task() {
    loop {
        if QUERY_SNAPSHOT.in_memory.is_empty() {
            break;
        }

        // Copy the next key and clone its buffer under a short shard read
        // guard. The in-memory entry is deliberately kept in place until the
        // on-disk commit succeeds: removing it first (as the old code did)
        // opened a window in which a reader saw neither the in-memory copy
        // nor the committed disk table, surfacing as "Unknown snapshot id".
        let (key, buffer) = {
            let _shard_probe = crate::tasks::txn_probe::guard_probe("flush_query_snapshot:33");
            let Some(entry_ref) = QUERY_SNAPSHOT.in_memory.iter().next() else {
                break;
            };
            (*entry_ref.key(), *entry_ref.value())
        };

        // Save to disk.
        let timer_start = Instant::now();
        let slot_guard = crate::tasks::txn_probe::slot_guard(
            crate::tasks::txn_probe::Db::Cache,
            "flush_query_snapshot:44",
        );
        let txn = match QUERY_SNAPSHOT.in_disk.begin_write() {
            Ok(t) => t,
            Err(e) => {
                handle_error(anyhow::anyhow!(
                    "FlushQuerySnapshotTask: Failed to begin write transaction: {e}"
                ));
                break;
            }
        };
        slot_guard.acquired();
        let count_version = &VERSION_COUNT_TIMESTAMP.load(Ordering::Relaxed).to_string();
        let table_definition: TableDefinition<u64, QueryCacheEntry> =
            TableDefinition::new(count_version);

        let mut table = {
            let opened = txn.open_table(table_definition);
            match opened {
                Ok(t) => t,
                Err(e) => {
                    handle_error(anyhow::anyhow!(
                        "FlushQuerySnapshotTask: Failed to open table {count_version}: {e}"
                    ));
                    break;
                }
            }
        };

        if let Err(e) = table.insert(key, buffer) {
            handle_error(anyhow::anyhow!(
                "FlushQuerySnapshotTask: Failed to insert data for expression_hashed {key}: {e}"
            ));
        }

        drop(table);
        if let Err(e) = txn.commit() {
            handle_error(anyhow::anyhow!(
                "FlushQuerySnapshotTask: Failed to commit transaction for expression_hashed {key}: {e}"
            ));
            break;
        }
        debug!(
            duration = &*format!("{:?}", timer_start.elapsed());
            "Write query cache into disk"
        );

        // Only now that the on-disk commit succeeded, drop the in-memory
        // entry. Unconditional removal of the same key is fine: snapshot
        // content is idempotent, so a concurrent/late writer winning the slot
        // is harmless.
        QUERY_SNAPSHOT.in_memory.remove(&key);

        debug!(
            "{} items remaining in in-memory query cache",
            QUERY_SNAPSHOT.in_memory.len()
        );
    }
}
