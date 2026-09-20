use crate::model::response::DatabaseTimestamp;
use crate::process::dir_album::drain_pending_album_updates;
use crate::storage::db::TREE;
use crate::storage::db::VERSION_COUNT_TIMESTAMP;
use crate::storage::db::open_data_table;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::actor::album::album_task;
use crate::tasks::batcher::update_expire::UpdateExpireTask;
use arrayvec::ArrayString;
use chrono::Utc;
use log::warn;
use mini_executor::BatchTask;
use rayon::iter::{ParallelBridge, ParallelIterator};
use rayon::prelude::ParallelSliceMut;
use redb::ReadableTable;
use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::Instant;

static ALLOWED_KEYS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "Make",
        "Model",
        "FNumber",
        "ExposureTime",
        "FocalLength",
        "PhotographicSensitivity",
        "DateTimeOriginal",
        "duration",
        "rotation",
    ]
    .iter()
    .copied()
    .collect()
});

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

    let database_timestamp_vec = build_from_asset_tables(&priority_list)
        .unwrap_or_else(|| build_from_data_table(&priority_list));

    let mut database_timestamp_vec = database_timestamp_vec;
    // Sort by timestamp descending, with a deterministic secondary key
    // (first alias path) so that items with equal timestamps have a stable
    // order. This prevents locate-by-hash from returning non-deterministic
    // results when multiple same-hash assets exist.
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

/// Build the in-memory tree from the legacy `DATA_TABLE` (one entry per hash).
fn build_from_data_table(priority_list: &[&str]) -> Vec<DatabaseTimestamp> {
    let data_table = open_data_table();

    data_table
        .iter()
        .expect("failed to iterate table")
        .par_bridge()
        .map(|guard| {
            let (_, value) = guard.expect("failed to read record");
            let mut abstract_data = value.value();
            if let Some(exif_vec) = abstract_data.exif_vec_mut() {
                exif_vec.retain(|k, _| ALLOWED_KEYS.contains(&k.as_str()));
            }
            DatabaseTimestamp::new(abstract_data, priority_list)
        })
        .collect()
}

/// Build the in-memory tree from `ASSET_BY_ID` (one entry per file/path).
/// Enriches with metadata from `DATA_TABLE` when available.
fn build_from_asset_tables(priority_list: &[&str]) -> Option<Vec<DatabaseTimestamp>> {
    use redb::{ReadableDatabase, ReadableTable};

    let Ok(txn) = TREE.in_disk.begin_read() else {
        return None;
    };
    let Ok(table) = txn.open_table(crate::storage::db::ASSET_BY_ID) else {
        return None;
    };

    let data_table = open_data_table();

    let mut entries = Vec::new();
    for row in table.iter().into_iter().flatten() {
        let Ok((_, value)) = row else { continue };
        let record: crate::model::asset::AssetRecord = match serde_json::from_str(value.value()) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let rich_data = data_table
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
/// is not available in `DATA_TABLE`.
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

/// Sync the path-primary asset tables (`ASSET_BY_PATH`, `ASSET_BY_ID`, `DUPE_INDEX`)
/// from the legacy `DATA_TABLE`. Each alias path gets its own asset record.
/// Uses deterministic asset IDs derived from the canonical path to avoid
/// flakiness across runs.
#[allow(dead_code)]
fn sync_asset_tables_from_data_table() {
    use crate::model::abstract_data::AbstractData;
    use crate::model::asset::{AssetKind, AssetRecord};
    use crate::storage::db::{ASSET_BY_ID, ASSET_BY_PATH, DUPE_INDEX};

    let data_table = open_data_table();

    // Clear existing asset tables.
    for table_def in [ASSET_BY_PATH, ASSET_BY_ID, DUPE_INDEX] {
        if let Ok(txn) = TREE.in_disk.begin_write() {
            if let Ok(table) = txn.open_table(table_def) {
                let keys: Vec<String> = table
                    .iter()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| r.ok().map(|(k, _)| k.value().to_string()))
                    .collect();
                drop(table);
                if let Ok(mut table) = txn.open_table(table_def) {
                    for key in &keys {
                        let _ = table.remove(key.as_str());
                    }
                }
            }
            let _ = txn.commit();
        }
    }

    // Populate from DATA_TABLE.
    let Ok(txn) = TREE.in_disk.begin_write() else {
        return;
    };
    let Ok(mut path_table) = txn.open_table(ASSET_BY_PATH) else {
        return;
    };
    let Ok(mut id_table) = txn.open_table(ASSET_BY_ID) else {
        return;
    };
    let Ok(mut dupe_table) = txn.open_table(DUPE_INDEX) else {
        return;
    };

    let Ok(iter) = data_table.iter() else {
        return;
    };

    for entry in iter.flatten() {
        let abstract_data = entry.1.value();
        let content_hash = abstract_data.hash();

        let (kind, aliases) = match &abstract_data {
            AbstractData::Image(img) => (AssetKind::Image, &img.metadata.alias),
            AbstractData::Video(vid) => (AssetKind::Video, &vid.metadata.alias),
            AbstractData::Album(_) => (AssetKind::Album, &Vec::new()),
        };

        for alias in aliases {
            // Derive deterministic asset_id from the canonical path.
            let asset_id = deterministic_id(&alias.file);

            let record = AssetRecord {
                asset_id,
                kind,
                canonical_path: alias.file.clone(),
                content_hash: Some(content_hash),
                file_size: 0,
                ext: String::new(),
                modified: alias.modified,
                scan_time: alias.scan_time,
                is_trashed: alias.is_trashed,
                album_id: None,
            };

            if let Ok(json) = serde_json::to_string(&record) {
                let _ = path_table.insert(alias.file.as_str(), &*asset_id);
                let _ = id_table.insert(&*asset_id, json.as_str());
            }

            // Add to DUPE_INDEX.
            let dupe_key = content_hash.as_str();
            let existing: Vec<String> = dupe_table
                .get(dupe_key)
                .ok()
                .flatten()
                .map(|g| serde_json::from_str(g.value()).unwrap_or_default())
                .unwrap_or_default();
            let mut ids = existing;
            if !ids.iter().any(|id| id == &*asset_id) {
                ids.push(asset_id.to_string());
                if let Ok(json) = serde_json::to_string(&ids) {
                    let _ = dupe_table.insert(dupe_key, json.as_str());
                }
            }
        }

        // Create album asset for directory albums.
        if let AbstractData::Album(album) = &abstract_data
            && !album.metadata.dir_path.is_empty()
        {
            let asset_id = deterministic_id(&album.metadata.dir_path);
            let record = AssetRecord::new_album(album.metadata.dir_path.clone());
            let record = AssetRecord { asset_id, ..record };
            if let Ok(json) = serde_json::to_string(&record) {
                let _ = path_table.insert(album.metadata.dir_path.as_str(), &*asset_id);
                let _ = id_table.insert(&*asset_id, json.as_str());
            }
        }
    }

    drop((path_table, id_table, dupe_table));
    let _ = txn.commit();
}

/// Generate a deterministic asset ID from a path string.
/// Uses blake3 hash of the path, truncated to 64 chars.
#[allow(dead_code)]
fn deterministic_id(path: &str) -> ArrayString<64> {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(path.as_bytes());
    let hash = hasher.finalize();
    let hex = hash.to_hex();
    ArrayString::from(&hex.as_str()[..64.min(hex.len())])
        .unwrap_or_else(|_| ArrayString::from("fallback").unwrap())
}
