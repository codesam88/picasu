use crate::model::response::ReducedData;
use crate::storage::cache::TREE_SNAPSHOT;
use mini_executor::BatchTask;
use redb::TableDefinition;
use std::time::Instant;

use crate::error::handle_error;
use anyhow;

pub struct FlushTreeSnapshotTask;

impl BatchTask for FlushTreeSnapshotTask {
    async fn batch_run(_: Vec<Self>) {
        flush_tree_snapshot_task();
    }
}

pub(crate) fn flush_tree_snapshot_task() {
    loop {
        if TREE_SNAPSHOT.in_memory.is_empty() {
            break;
        }

        // Copy the next key and clone its buffer under a short shard read
        // guard. The in-memory entry is deliberately kept in place until the
        // on-disk commit succeeds: removing it first (as the old code did)
        // opened a window in which a reader saw neither the in-memory copy
        // nor the committed disk table, surfacing as "Unknown snapshot id".
        let (timestamp, buffer) = {
            let _shard_probe = crate::tasks::txn_probe::guard_probe("flush_tree_snapshot:30");
            let Some(entry_ref) = TREE_SNAPSHOT.in_memory.iter().next() else {
                break;
            };
            (*entry_ref.key(), entry_ref.value().clone())
        };

        let timestamp_str = timestamp.to_string();

        let timer_start = Instant::now();
        let slot_guard = crate::tasks::txn_probe::slot_guard(
            crate::tasks::txn_probe::Db::Tree,
            "flush_tree_snapshot:42",
        );
        let txn = match TREE_SNAPSHOT.in_disk.begin_write() {
            Ok(t) => t,
            Err(e) => {
                handle_error(anyhow::anyhow!(
                    "FlushTreeSnapshotTask: Failed to begin write transaction: {e}"
                ));
                break;
            }
        };
        slot_guard.acquired();
        let table_definition: TableDefinition<u64, ReducedData> =
            TableDefinition::new(&timestamp_str);

        let mut table = {
            let opened = txn.open_table(table_definition);
            match opened {
                Ok(t) => t,
                Err(e) => {
                    handle_error(anyhow::anyhow!(
                        "FlushTreeSnapshotTask: Failed to open table {timestamp_str}: {e}"
                    ));
                    break;
                }
            }
        };
        for (index, data) in buffer.iter().enumerate() {
            if let Err(e) = table.insert(index as u64, *data) {
                handle_error(anyhow::anyhow!(
                    "FlushTreeSnapshotTask: Failed to insert data at index {index} for timestamp {timestamp}: {e}"
                ));
            }
        }

        drop(table);
        if let Err(e) = txn.commit() {
            handle_error(anyhow::anyhow!(
                "FlushTreeSnapshotTask: Failed to commit transaction for timestamp {timestamp}: {e}"
            ));
            break;
        }

        debug!(
            duration = &*format!("{:?}", timer_start.elapsed());
            "Write in-memory cache into disk"
        );

        // Only now that the on-disk commit succeeded, drop the in-memory
        // entry. Unconditional removal of the same key is fine: snapshot
        // content is idempotent, so a concurrent/late writer winning the slot
        // is harmless.
        TREE_SNAPSHOT.in_memory.remove(&timestamp);

        debug!(
            "{} items remaining in in-memory tree cache",
            TREE_SNAPSHOT.in_memory.len()
        );
    }
}
