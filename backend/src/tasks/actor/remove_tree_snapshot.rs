use crate::model::response::ReducedData;
use crate::storage::cache::TREE_SNAPSHOT;
use redb::TableDefinition;

/// Removes a tree cache table by its timestamp.
pub(crate) fn remove_task(timestamp: i64) {
    let write_txn = TREE_SNAPSHOT
        .in_disk
        .begin_write()
        .expect("failed to begin write transaction");
    let binding = timestamp.to_string();
    let table_definition: TableDefinition<u64, ReducedData> = TableDefinition::new(&binding);

    match write_txn.delete_table(table_definition) {
        Ok(true) => {
            log::debug!("Delete tree cache table: {timestamp:?}");
        }
        Ok(false) => {
            log::error!("Failed to delete tree cache table: {timestamp:?}");
        }
        Err(err) => {
            log::error!("Failed to delete tree cache table: {timestamp:?}, error: {err:#?}");
        }
    }

    log::debug!(
        "{} items remaining in disk tree cache",
        write_txn
            .list_tables()
            .expect("failed to list tables")
            .count()
    );

    write_txn.commit().expect("failed to commit transaction");
}
