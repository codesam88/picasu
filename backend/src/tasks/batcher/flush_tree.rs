use mini_executor::BatchTask;
use std::path::Path;

use arrayvec::ArrayString;

use crate::{
    model::abstract_data::AbstractData,
    model::asset::{AssetKind, AssetRecord},
    process::dir_album::{mark_album_for_update, mark_dir_albums_for_path},
    storage::db::{ASSET_BY_ID, ASSET_BY_PATH, DUPE_INDEX, METADATA_TABLE, TREE},
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

fn flush_tree_task(insert_list: &[AbstractData], remove_list: &[AbstractData]) {
    flush_tables(insert_list, remove_list);
    BATCH_COORDINATOR.execute_batch_detached(UpdateTreeTask);
}

/// Write the insert/remove lists to the four asset tables (`ASSET_BY_ID`,
/// `ASSET_BY_PATH`, `DUPE_INDEX`, `METADATA_TABLE`).
///
/// Does not dispatch [`UpdateTreeTask`]; `flush_tree_task` does that after
/// the tables are written. Tests call this directly so no background tree
/// rebuild races their assertions.
#[allow(clippy::too_many_lines)]
fn flush_tables(insert_list: &[AbstractData], remove_list: &[AbstractData]) {
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
            .path()
            .map(|a| a.file.clone())
            .unwrap_or_default();

        // Check if this path already has an asset (idempotent re-index).
        let existing_id =
            if let Ok(Some(existing_id)) = asset_store::get_asset_id_by_path(&canonical_path) {
                Some(existing_id)
            } else {
                None
            };
        // Read the previous content hash before `begin_write`: store helpers
        // open their own transactions, which cannot run inside the flush
        // write transaction (redb is single-writer).
        let old_hash = existing_id
            .as_ref()
            .and_then(|id| asset_store::get_asset_by_id(id).ok().flatten())
            .and_then(|record| record.content_hash);
        let asset_id = existing_id.unwrap_or_else(generate_random_hash);

        let kind = match abstract_data {
            AbstractData::Image(_) => AssetKind::Image,
            AbstractData::Video(_) => AssetKind::Video,
            AbstractData::Album(_) => AssetKind::Album,
        };

        let modified = abstract_data.path().map_or(0, |a| a.modified);
        // Carry the trash flag from the flushed data (albums store it on the
        // record, media on the file entry) so lean list rows derived from this
        // record see the same trashed state as the metadata row.
        let is_trashed = match abstract_data {
            AbstractData::Album(alb) => alb.metadata.is_trashed,
            _ => abstract_data.path().is_some_and(|a| a.is_trashed),
        };
        // Mirror the stored path's scan_time (index time) rather than stamping
        // "now", so re-flushing on a metadata edit does not rewrite identity
        // times.
        let scan_time = abstract_data
            .path()
            .map_or_else(|| chrono::Utc::now().timestamp_millis(), |a| a.scan_time);
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
            scan_time,
            is_trashed,
            album_id: abstract_data.album(),
        };

        // Write to ASSET_BY_ID, ASSET_BY_PATH, DUPE_INDEX, and METADATA_TABLE.
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
                let mut metadata_table = write_txn
                    .open_table(METADATA_TABLE)
                    .expect("failed to open METADATA_TABLE");

                // Write to ASSET_BY_ID and ASSET_BY_PATH.
                id_table
                    .insert(&*asset_id, json.as_str())
                    .expect("failed to insert into ASSET_BY_ID");
                path_table
                    .insert(canonical_path.as_str(), &*asset_id)
                    .expect("failed to insert into ASSET_BY_PATH");

                // Write full AbstractData to METADATA_TABLE keyed by asset_id.
                metadata_table
                    .insert(&*asset_id, abstract_data)
                    .expect("failed to insert into METADATA_TABLE");

                log::info!(
                    "flush_tree: wrote to METADATA_TABLE key={asset_id}, path={canonical_path}, content_hash={content_hash}"
                );

                // When the file's bytes changed, first drop this asset from its
                // previous hash group so the old group does not keep stale
                // membership.
                if let Some(old) = old_hash.as_ref()
                    && old != &content_hash
                {
                    remove_from_old_group(&mut dupe_table, old, &asset_id);
                }

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
        if let Some(file_entry) = abstract_data.path() {
            mark_dir_albums_for_path(Path::new(&file_entry.file));
        }
    }

    // Process removes: delete from asset tables.
    for abstract_data in remove_list {
        let canonical_path = abstract_data
            .path()
            .map(|a| a.file.clone())
            .unwrap_or_default();

        if canonical_path.is_empty() {
            // Path pruned (e.g., by sweep_stale_asset_paths) — `None` maps to
            // an empty canonical path. Remove any asset in the DUPE_INDEX
            // group whose canonical path no longer exists on disk.
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
            // Normal case: the record holds its canonical path — remove that
            // specific asset.
            if let Ok(Some(asset_id)) = asset_store::get_asset_id_by_path(&canonical_path) {
                remove_asset_from_tables(&asset_id, &canonical_path, abstract_data.hash());
            }
        }

        if let Some(album_id) = abstract_data.album() {
            mark_album_for_update(album_id);
        }
        if let Some(file_entry) = abstract_data.path() {
            mark_dir_albums_for_path(Path::new(&file_entry.file));
        }
    }
}

/// Drop `asset_id` from its previous content-hash group (`old_hash`) using
/// the flush transaction's own `DUPE_INDEX` handle.
///
/// Mirrors [`crate::storage::asset_store::remove_from_dupe_group`], but that
/// function opens its own write transaction — calling it here would deadlock,
/// because redb permits only one open write transaction at a time and the
/// flush transaction is already active.
fn remove_from_old_group(
    dupe_table: &mut redb::Table<'_, &str, &str>,
    old_hash: &ArrayString<64>,
    asset_id: &ArrayString<64>,
) {
    use redb::ReadableTable;

    let prior: Vec<String> = dupe_table
        .get(old_hash.as_str())
        .ok()
        .flatten()
        .map(|g| serde_json::from_str(g.value()).unwrap_or_default())
        .unwrap_or_default();
    let retained: Vec<String> = prior
        .into_iter()
        .filter(|id| id.as_str() != asset_id.as_str())
        .collect();
    if retained.is_empty() {
        dupe_table
            .remove(old_hash.as_str())
            .expect("failed to remove old DUPE_INDEX group");
    } else {
        let ids_json = serde_json::to_string(&retained).expect("failed to serialize dupe IDs");
        dupe_table
            .insert(old_hash.as_str(), ids_json.as_str())
            .expect("failed to update old DUPE_INDEX group");
    }
}

/// Remove an asset from all tables: `ASSET_BY_ID`, `ASSET_BY_PATH`, `DUPE_INDEX`, `METADATA_TABLE`.
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
        let mut metadata_table = write_txn
            .open_table(METADATA_TABLE)
            .expect("failed to open METADATA_TABLE");
        let _ = metadata_table.remove(&**asset_id);
    }
    let _ = write_txn.commit();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::hash::blake3_hasher;
    use crate::storage::asset_store;
    use crate::tests::bootstrap::*;
    use redb::{ReadableDatabase, ReadableTable, TableDefinition};
    use std::path::PathBuf;

    fn clear_table<V: redb::Value>(table_def: TableDefinition<'static, &'static str, V>) {
        let txn = TREE.in_disk.begin_write().expect("begin write for clear");
        {
            let table = txn.open_table(table_def).expect("open table for clear");
            let keys: Vec<String> = table
                .iter()
                .expect("iterate")
                .filter_map(|r| r.ok().map(|(k, _)| k.value().to_string()))
                .collect();
            drop(table);
            let mut table = txn.open_table(table_def).expect("reopen table for clear");
            for key in &keys {
                table.remove(key.as_str()).expect("remove key");
            }
        }
        txn.commit().expect("commit clear");
    }

    /// Flush writes all four tables, so tests must clear all four before
    /// asserting — not just the three asset tables.
    fn clear_all_tables() {
        clear_table(ASSET_BY_PATH);
        clear_table(ASSET_BY_ID);
        clear_table(DUPE_INDEX);
        clear_table(METADATA_TABLE);
    }

    fn metadata_table_key_count() -> usize {
        let txn = TREE
            .in_disk
            .begin_read()
            .expect("begin read METADATA_TABLE");
        let table = txn.open_table(METADATA_TABLE).expect("open METADATA_TABLE");
        table.iter().expect("iterate METADATA_TABLE").count()
    }

    fn make_jpeg(dir: &Path, name: &str, side: u32) -> PathBuf {
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(dir.join(name).to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(side),
            height: Some(side),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .expect("generate jpeg");
        dir.join(name)
    }

    fn hash_file(path: &Path) -> ArrayString<64> {
        let file = std::fs::File::open(path).expect("open file for hashing");
        blake3_hasher(file).expect("hash file")
    }

    fn asset_id_at(path: &Path) -> ArrayString<64> {
        asset_store::get_asset_id_by_path(&path.to_string_lossy())
            .expect("lookup ASSET_BY_PATH")
            .expect("asset must exist at path")
    }

    #[test]
    fn flush_identical_bytes_two_paths_two_assets() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        clear_all_tables();

        let dir = test_image_home().join("flush_dup");
        std::fs::create_dir_all(&dir).unwrap();

        let path_a = make_jpeg(&dir, "a.jpg", 4);
        std::fs::copy(&path_a, dir.join("b.jpg")).unwrap();
        let path_b = dir.join("b.jpg");

        let hash_a = hash_file(&path_a);
        let hash_b = hash_file(&path_b);
        assert_eq!(hash_a, hash_b, "identical bytes must hash the same");

        let data_a = AbstractData::new(&path_a, hash_a).expect("build AbstractData for a.jpg");
        let data_b = AbstractData::new(&path_b, hash_b).expect("build AbstractData for b.jpg");

        flush_tables(&[data_a, data_b], &[]);

        let id_a = asset_id_at(&path_a);
        let id_b = asset_id_at(&path_b);
        assert_ne!(id_a, id_b, "two paths must get distinct asset IDs");

        let ids = asset_store::get_dupe_ids(&hash_a).expect("read DUPE_INDEX");
        assert_eq!(ids.len(), 2, "same-hash group must hold both assets");
        assert!(ids.contains(&id_a));
        assert!(ids.contains(&id_b));

        clear_all_tables();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn flush_same_path_idempotent() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        clear_all_tables();

        let dir = test_image_home().join("flush_idem");
        std::fs::create_dir_all(&dir).unwrap();

        let path = make_jpeg(&dir, "photo.jpg", 4);
        let hash = hash_file(&path);
        let data = AbstractData::new(&path, hash).expect("build AbstractData");

        flush_tables(&[data.clone()], &[]);
        let id_first = asset_id_at(&path);

        flush_tables(&[data], &[]);
        let id_second = asset_id_at(&path);

        assert_eq!(id_first, id_second, "same path must keep the same asset ID");

        let ids = asset_store::get_dupe_ids(&hash).expect("read DUPE_INDEX");
        assert_eq!(ids.len(), 1, "re-flush must not grow the dupe group");
        assert_eq!(ids[0], id_first);
        assert_eq!(
            metadata_table_key_count(),
            1,
            "re-flush must not leave duplicate METADATA_TABLE keys"
        );

        clear_all_tables();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn flush_changed_hash_moves_dupe_group() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        clear_all_tables();

        let dir = test_image_home().join("flush_changed");
        std::fs::create_dir_all(&dir).unwrap();

        let path = make_jpeg(&dir, "photo.jpg", 4);
        let hash_a = hash_file(&path);
        let hash_b = ArrayString::from("changed_bytes_hash_b").expect("fits in 64 chars");
        assert_ne!(hash_a, hash_b, "test needs two distinct hashes");

        let data_a = AbstractData::new(&path, hash_a).expect("build AbstractData, hash A");
        flush_tables(&[data_a], &[]);
        let id_first = asset_id_at(&path);

        let data_b = AbstractData::new(&path, hash_b).expect("build AbstractData, hash B");
        flush_tables(&[data_b], &[]);
        let id_second = asset_id_at(&path);

        assert_eq!(id_first, id_second, "same path must keep the same asset ID");

        let group_a = asset_store::get_dupe_ids(&hash_a).expect("read group A");
        assert!(
            !group_a.contains(&id_first),
            "old-hash group must no longer contain the asset"
        );
        let group_b = asset_store::get_dupe_ids(&hash_b).expect("read group B");
        assert_eq!(
            group_b,
            vec![id_second],
            "new-hash group must contain exactly the asset"
        );

        clear_all_tables();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn flush_remove_one_same_hash_leaves_other() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        clear_all_tables();

        let dir = test_image_home().join("flush_remove");
        std::fs::create_dir_all(&dir).unwrap();

        let path_a = make_jpeg(&dir, "a.jpg", 4);
        std::fs::copy(&path_a, dir.join("b.jpg")).unwrap();
        let path_b = dir.join("b.jpg");

        let hash = hash_file(&path_a);
        let data_a = AbstractData::new(&path_a, hash).expect("build AbstractData for a.jpg");
        let data_b = AbstractData::new(&path_b, hash).expect("build AbstractData for b.jpg");

        flush_tables(&[data_a.clone(), data_b], &[]);
        let id_a = asset_id_at(&path_a);
        let id_b = asset_id_at(&path_b);
        assert_ne!(id_a, id_b);

        // The normal remove branch resolves by canonical path via
        // ASSET_BY_PATH and does not consult the filesystem, so the
        // remove input is the same path AbstractData used for the insert.
        flush_tables(&[], &[data_a]);

        let found_b = asset_store::get_asset_by_id(id_b.as_str())
            .expect("read ASSET_BY_ID")
            .expect("asset B must survive removal of asset A");
        assert_eq!(found_b.asset_id, id_b);

        let ids = asset_store::get_dupe_ids(&hash).expect("read DUPE_INDEX");
        assert_eq!(ids.len(), 1, "group must keep only the surviving asset");
        assert!(ids.contains(&id_b));
        assert!(!ids.contains(&id_a));

        assert!(
            asset_store::get_asset_by_id(id_a.as_str())
                .expect("read ASSET_BY_ID")
                .is_none(),
            "asset A must be gone from ASSET_BY_ID"
        );
        assert!(
            asset_store::get_asset_id_by_path(&path_a.to_string_lossy())
                .expect("lookup ASSET_BY_PATH")
                .is_none(),
            "asset A path mapping must be gone"
        );
        assert_eq!(
            metadata_table_key_count(),
            1,
            "only B's METADATA_TABLE row remains"
        );

        clear_all_tables();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
