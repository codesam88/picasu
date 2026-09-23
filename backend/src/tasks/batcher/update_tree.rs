use crate::model::response::DatabaseTimestamp;
use crate::process::dir_album::drain_pending_album_updates;
use crate::storage::db::TREE;
use crate::storage::db::VERSION_COUNT_TIMESTAMP;
use crate::storage::db::open_metadata_table;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::actor::album::album_task;
use crate::tasks::batcher::update_expire::UpdateExpireTask;
use chrono::Utc;
use log::warn;
use mini_executor::BatchTask;
use rayon::prelude::ParallelSliceMut;
use std::time::Instant;

pub struct UpdateTreeTask;

impl BatchTask for UpdateTreeTask {
    async fn batch_run(_: Vec<Self>) {
        update_tree_task();

        // Run self-updates for any albums whose members were recently changed.
        let pending = drain_pending_album_updates();
        if !pending.is_empty() {
            for album_id in pending {
                if let Err(e) = tokio::task::spawn_blocking(move || album_task(album_id)).await {
                    warn!("Album self-update task panicked for {album_id}: {e}");
                }
            }
            // Refresh the in-memory tree so updated album stats are visible.
            update_tree_task();
        }
    }
}

fn update_tree_task() {
    let start_time = Instant::now();

    let priority_list = vec!["DateTimeOriginal", "filename", "modified", "scan_time"];

    let mut database_timestamp_vec = build_from_asset_tables(&priority_list).unwrap_or_default();

    // Sort by timestamp descending, with a deterministic secondary key
    // (the asset path) so that items with equal timestamps have a
    // stable order.
    database_timestamp_vec.par_sort_by(|a, b| {
        b.timestamp.cmp(&a.timestamp).then_with(|| {
            let a_path = a.abstract_data.path().map_or("", |a| a.file.as_str());
            let b_path = b.abstract_data.path().map_or("", |a| a.file.as_str());
            a_path.cmp(b_path)
        })
    });

    *TREE.in_memory.write().expect("lock poisoned") = database_timestamp_vec;

    // Update VERSION_COUNT_TIMESTAMP immediately so query caches are
    // invalidated before the next prefetch call.
    let current_timestamp = Utc::now().timestamp_millis();
    VERSION_COUNT_TIMESTAMP.store(current_timestamp, std::sync::atomic::Ordering::SeqCst);

    BATCH_COORDINATOR.execute_batch_detached(UpdateExpireTask);

    let duration = format!("{:?}", start_time.elapsed());
    info!(duration = &*duration; "In-memory cache updated ({}).", current_timestamp);
}

/// Build the in-memory tree from `ASSET_BY_ID` (one entry per file/path),
/// composing each row from the asset's identity `AssetRecord` plus its
/// optional `METADATA_TABLE` payload.
fn build_from_asset_tables(priority_list: &[&str]) -> Option<Vec<DatabaseTimestamp>> {
    use crate::model::metadata_record::compose_abstract_data;
    use redb::{ReadableDatabase, ReadableTable};

    let Ok(txn) = TREE.in_disk.begin_read() else {
        log::info!("build_from_asset_tables: failed to begin read transaction");
        return None;
    };
    let Ok(table) = txn.open_table(crate::storage::db::ASSET_BY_ID) else {
        log::info!("build_from_asset_tables: failed to open ASSET_BY_ID");
        return None;
    };

    let metadata_table = open_metadata_table();

    let mut entries = Vec::new();
    for row in table.iter().into_iter().flatten() {
        let Ok((_, value)) = row else { continue };
        let record: crate::model::asset::AssetRecord = match serde_json::from_str(value.value()) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let payload = metadata_table
            .get(&*record.asset_id)
            .ok()
            .flatten()
            .map(|g| g.value());

        // Composition makes path drift impossible: the file entry is always
        // assembled from the record, never read back from storage.
        let abstract_data = compose_abstract_data(&record, payload.as_ref());

        entries.push(DatabaseTimestamp::with_asset_id(
            abstract_data,
            priority_list,
            record.asset_id,
        ));
    }

    log::info!(
        "build_from_asset_tables: built {} entries from ASSET_BY_ID",
        entries.len()
    );

    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}
