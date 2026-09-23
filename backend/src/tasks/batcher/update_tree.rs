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
    // (first alias path) so that items with equal timestamps have a stable
    // order.
    database_timestamp_vec.par_sort_by(|a, b| {
        b.timestamp.cmp(&a.timestamp).then_with(|| {
            let a_path = a
                .abstract_data
                .alias()
                .first()
                .map_or("", |a| a.file.as_str());
            let b_path = b
                .abstract_data
                .alias()
                .first()
                .map_or("", |a| a.file.as_str());
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

/// Build the in-memory tree from `ASSET_BY_ID` (one entry per file/path).
/// Enriches with metadata from `METADATA_TABLE` when available.
fn build_from_asset_tables(priority_list: &[&str]) -> Option<Vec<DatabaseTimestamp>> {
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

        let rich_data = metadata_table
            .get(&*record.asset_id)
            .ok()
            .flatten()
            .map(|g| g.value());

        let abstract_data = if let Some(mut data) = rich_data {
            trim_aliases_to_path(&mut data, &record);
            data
        } else {
            minimal_abstract_data(&record)
        };

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

/// Trim an `AbstractData` record's aliases to only the given asset's path.
fn trim_aliases_to_path(
    data: &mut crate::model::abstract_data::AbstractData,
    record: &crate::model::asset::AssetRecord,
) {
    use crate::model::abstract_data::AbstractData;
    use crate::model::response::FileModify;
    let alias = FileModify {
        file: record.canonical_path.clone(),
        modified: record.modified,
        scan_time: record.scan_time,
        is_trashed: record.is_trashed,
    };
    match data {
        AbstractData::Image(img) => {
            img.metadata
                .alias
                .retain(|a| a.file == record.canonical_path);
            if img.metadata.alias.is_empty() {
                img.metadata.alias.push(alias);
            }
        }
        AbstractData::Video(vid) => {
            vid.metadata
                .alias
                .retain(|a| a.file == record.canonical_path);
            if vid.metadata.alias.is_empty() {
                vid.metadata.alias.push(alias);
            }
        }
        AbstractData::Album(_) => {}
    }
}

/// Create a minimal `AbstractData` from an `AssetRecord` when rich metadata
/// is not available in `METADATA_TABLE`.
fn minimal_abstract_data(
    record: &crate::model::asset::AssetRecord,
) -> crate::model::abstract_data::AbstractData {
    use crate::model::abstract_data::AbstractData;
    use crate::model::response::FileModify;
    let display_id = record.content_hash.unwrap_or(record.asset_id);
    let object = crate::model::object::ObjectSchema::new(
        display_id,
        match record.kind {
            crate::model::asset::AssetKind::Image => crate::model::object::ObjectType::Image,
            crate::model::asset::AssetKind::Video => crate::model::object::ObjectType::Video,
            crate::model::asset::AssetKind::Album => crate::model::object::ObjectType::Album,
        },
    );
    let alias = FileModify {
        file: record.canonical_path.clone(),
        modified: record.modified,
        scan_time: record.scan_time,
        is_trashed: record.is_trashed,
    };
    match record.kind {
        crate::model::asset::AssetKind::Image => {
            let mut metadata = crate::model::image::ImageMetadata::new(
                display_id,
                record.file_size,
                0,
                0,
                record.ext.clone(),
            );
            metadata.alias = vec![alias];
            AbstractData::Image(crate::model::image::ImageCombined { object, metadata })
        }
        crate::model::asset::AssetKind::Video => {
            let mut metadata = crate::model::video::VideoMetadata::new(
                display_id,
                record.file_size,
                0,
                0,
                record.ext.clone(),
            );
            metadata.alias = vec![alias];
            AbstractData::Video(crate::model::video::VideoCombined { object, metadata })
        }
        crate::model::asset::AssetKind::Album => {
            let metadata = crate::model::album::AlbumMetadata {
                id: display_id,
                dir_path: record.canonical_path.clone(),
                ..Default::default()
            };
            AbstractData::Album(crate::model::album::AlbumCombined { object, metadata })
        }
    }
}
