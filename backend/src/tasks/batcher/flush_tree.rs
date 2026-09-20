use mini_executor::BatchTask;
use std::path::Path;

use arrayvec::ArrayString;

use crate::{
    model::abstract_data::AbstractData,
    model::asset::{AssetKind, AssetRecord},
    process::dir_album::{mark_album_for_update, mark_dir_albums_for_path},
    storage::db::{ASSET_BY_ID, ASSET_BY_PATH, DATA_TABLE, DUPE_INDEX, TREE},
    tasks::{BATCH_COORDINATOR, batcher::update_tree::UpdateTreeTask},
};

pub struct FlushTreeTask {
    pub insert_list: Vec<AbstractData>,
    pub remove_list: Vec<AbstractData>,
}

impl FlushTreeTask {
    pub fn insert(data_list: Vec<AbstractData>) -> Self {
        Self {
            insert_list: data_list,
            remove_list: Vec::new(),
        }
    }

    pub fn remove(abstract_data_list: Vec<AbstractData>) -> Self {
        Self {
            insert_list: Vec::new(),
            remove_list: abstract_data_list,
        }
    }
}

impl BatchTask for FlushTreeTask {
    async fn batch_run(list: Vec<Self>) {
        let mut all_insert_data = Vec::new();
        let mut all_remove_abstract_data = Vec::new();
        for task in list {
            all_insert_data.extend(task.insert_list);
            all_remove_abstract_data.extend(task.remove_list);
        }
        flush_tree_task(&all_insert_data, &all_remove_abstract_data);
    }
}

#[allow(clippy::too_many_lines)]
fn flush_tree_task(insert_list: &[AbstractData], remove_list: &[AbstractData]) {
    use crate::process::hash::generate_random_hash;
    use crate::storage::asset_store;
    use redb::ReadableTable;

    log::info!(
        "flush_tree_task: {} inserts, {} removes",
        insert_list.len(),
        remove_list.len()
    );

    // Process inserts: each AbstractData gets its own asset record.
    for abstract_data in insert_list {
        let content_hash = abstract_data.hash();
        let canonical_path = abstract_data
            .alias()
            .first()
            .map(|a| a.file.clone())
            .unwrap_or_default();

        // Check if this path already has an asset (idempotent re-index).
        let asset_id =
            if let Ok(Some(existing_id)) = asset_store::get_asset_id_by_path(&canonical_path) {
                existing_id
            } else {
                generate_random_hash()
            };

        let kind = match abstract_data {
            AbstractData::Image(_) => AssetKind::Image,
            AbstractData::Video(_) => AssetKind::Video,
            AbstractData::Album(_) => AssetKind::Album,
        };

        let modified = abstract_data.alias().first().map_or(0, |a| a.modified);
        let ext = match abstract_data {
            AbstractData::Image(img) => img.metadata.ext.clone(),
            AbstractData::Video(vid) => vid.metadata.ext.clone(),
            AbstractData::Album(_) => String::new(),
        };
        let file_size = match abstract_data {
            AbstractData::Image(img) => img.metadata.size,
            AbstractData::Video(vid) => vid.metadata.size,
            AbstractData::Album(_) => 0,
        };

        let record = AssetRecord {
            asset_id,
            kind,
            canonical_path: canonical_path.clone(),
            content_hash: Some(content_hash),
            file_size,
            ext,
            modified,
            scan_time: chrono::Utc::now().timestamp_millis(),
            is_trashed: false,
            album_id: abstract_data.album(),
        };

        // Write to ASSET_BY_ID, ASSET_BY_PATH, DUPE_INDEX, and DATA_TABLE.
        if let Ok(json) = serde_json::to_string(&record) {
            let write_txn = TREE
                .in_disk
                .begin_write()
                .expect("failed to begin write transaction for asset flush");
            {
                let mut id_table = write_txn
                    .open_table(ASSET_BY_ID)
                    .expect("failed to open ASSET_BY_ID");
                let mut path_table = write_txn
                    .open_table(ASSET_BY_PATH)
                    .expect("failed to open ASSET_BY_PATH");
                let mut dupe_table = write_txn
                    .open_table(DUPE_INDEX)
                    .expect("failed to open DUPE_INDEX");
                let mut data_table = write_txn
                    .open_table(DATA_TABLE)
                    .expect("failed to open DATA_TABLE");

                // Write to ASSET_BY_ID and ASSET_BY_PATH.
                id_table
                    .insert(&*asset_id, json.as_str())
                    .expect("failed to insert into ASSET_BY_ID");
                path_table
                    .insert(canonical_path.as_str(), &*asset_id)
                    .expect("failed to insert into ASSET_BY_PATH");

                // Write full AbstractData to DATA_TABLE keyed by asset_id.
                data_table
                    .insert(&*asset_id, abstract_data)
                    .expect("failed to insert into DATA_TABLE");

                log::info!(
                    "flush_tree: wrote to DATA_TABLE key={asset_id}, path={canonical_path}, content_hash={content_hash}"
                );

                // Update DUPE_INDEX: add this asset_id to the content hash group.
                let dupe_key = &*content_hash;
                let existing: Vec<String> = dupe_table
                    .get(dupe_key)
                    .ok()
                    .flatten()
                    .map(|g| serde_json::from_str(g.value()).unwrap_or_default())
                    .unwrap_or_default();
                let mut ids = existing;
                if !ids.iter().any(|id| id == &*asset_id) {
                    ids.push(asset_id.to_string());
                    if let Ok(ids_json) = serde_json::to_string(&ids) {
                        dupe_table
                            .insert(dupe_key, ids_json.as_str())
                            .expect("failed to update DUPE_INDEX");
                    }
                }
            }
            write_txn.commit().expect("failed to commit asset flush");
        }

        if let Some(album_id) = abstract_data.album() {
            mark_album_for_update(album_id);
        }
        for file_modify in abstract_data.alias() {
            mark_dir_albums_for_path(Path::new(&file_modify.file));
        }
    }

    // Process removes: delete from asset tables.
    for abstract_data in remove_list {
        let canonical_path = abstract_data
            .alias()
            .first()
            .map(|a| a.file.clone())
            .unwrap_or_default();

        if canonical_path.is_empty() {
            // Alias list was pruned (e.g., by sweep_stale_aliases).
            // Remove any asset in the DUPE_INDEX group whose canonical path
            // no longer exists on disk.
            let content_hash = abstract_data.hash();
            if let Ok(ids) = asset_store::get_dupe_ids(&content_hash) {
                for id in ids {
                    if let Ok(Some(record)) = asset_store::get_asset_by_id(id.as_ref())
                        && !std::path::Path::new(&record.canonical_path).exists()
                    {
                        remove_asset_from_tables(&id, &record.canonical_path, content_hash);
                    }
                }
            }
        } else {
            // Normal case: alias list has a path — remove that specific asset.
            if let Ok(Some(asset_id)) = asset_store::get_asset_id_by_path(&canonical_path) {
                remove_asset_from_tables(&asset_id, &canonical_path, abstract_data.hash());
            }
        }

        if let Some(album_id) = abstract_data.album() {
            mark_album_for_update(album_id);
        }
        for file_modify in abstract_data.alias() {
            mark_dir_albums_for_path(Path::new(&file_modify.file));
        }
    }

    BATCH_COORDINATOR.execute_batch_detached(UpdateTreeTask);
}

/// Remove an asset from all tables: `ASSET_BY_ID`, `ASSET_BY_PATH`, `DUPE_INDEX`, `DATA_TABLE`.
fn remove_asset_from_tables(
    asset_id: &ArrayString<64>,
    canonical_path: &str,
    content_hash: ArrayString<64>,
) {
    use crate::storage::asset_store;

    let _ = asset_store::remove_asset_by_id(asset_id);
    let _ = asset_store::remove_asset_by_path(canonical_path);
    let _ = asset_store::remove_from_dupe_group(&content_hash, *asset_id);

    let write_txn = TREE
        .in_disk
        .begin_write()
        .expect("failed to begin write transaction for asset remove");
    {
        let mut data_table = write_txn
            .open_table(DATA_TABLE)
            .expect("failed to open DATA_TABLE");
        let _ = data_table.remove(&**asset_id);
    }
    let _ = write_txn.commit();
}
