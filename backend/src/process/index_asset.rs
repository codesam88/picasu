#![allow(dead_code)]

use anyhow::{Context, Result};
use log::info;
use std::path::Path;

use crate::model::asset::{AssetKind, AssetRecord, canonicalize_path};
use crate::model::media::is_valid_media_file;
use crate::process::hash::blake3_hasher;
use crate::storage::asset_store;

/// Index a single media file using the path-primary asset model.
///
/// The file is identified by its canonical path, not by its content hash.
/// If the path already has an asset, the existing record is updated (hash,
/// file size, modified time). If the path is new, a fresh asset ID is
/// allocated. The hash group (`DUPE_INDEX`) is updated independently.
///
/// Returns the `AssetRecord` for the indexed file.
pub fn index_asset(src: &Path, image_root: &Path) -> Result<AssetRecord> {
    let canonical = canonicalize_path(src, image_root);
    let canonical_str = canonical.to_string_lossy().into_owned();

    if !src.exists() {
        anyhow::bail!("File does not exist: {}", src.display());
    }

    if !is_valid_media_file(src) {
        anyhow::bail!("Not a valid media file: {}", src.display());
    }

    // Compute content hash.
    let file =
        std::fs::File::open(src).with_context(|| format!("Failed to open {}", src.display()))?;
    let content_hash =
        blake3_hasher(file).with_context(|| format!("Failed to hash {}", src.display()))?;

    // Get file metadata.
    let md = std::fs::metadata(src)
        .with_context(|| format!("Failed to read metadata for {}", src.display()))?;
    let file_size = md.len();
    let modified = md.modified().map_or(0, |t| {
        let millis = t
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        i64::try_from(millis).unwrap_or(0)
    });

    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let kind = if crate::constant::VALID_IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        AssetKind::Image
    } else {
        AssetKind::Video
    };

    // Look up by canonical path first.
    let existing_id = asset_store::get_asset_id_by_path(&canonical_str)?;

    let record = if let Some(asset_id) = existing_id {
        // Path already has an asset. Update it.
        let mut record = asset_store::get_asset_by_id(&asset_id)?
            .ok_or_else(|| anyhow::anyhow!("Asset {asset_id} not found in ASSET_BY_ID"))?;

        let old_hash = record.content_hash;

        // Update file-derived fields.
        record.content_hash = Some(content_hash);
        record.file_size = file_size;
        record.modified = modified;
        record.ext = ext;

        // If hash changed, update DUPE_INDEX: remove from old group, add to new.
        if old_hash.as_ref() != Some(&content_hash) {
            if let Some(old) = old_hash {
                asset_store::remove_from_dupe_group(&old, asset_id)?;
            }
            asset_store::add_to_dupe_group(&content_hash, asset_id)?;
        }

        asset_store::put_asset_by_id(&record)?;
        record
    } else {
        // New path — allocate a fresh asset.
        let mut record = AssetRecord::new_media(
            kind,
            canonical_str.clone(),
            Some(content_hash),
            file_size,
            ext,
            modified,
        );

        // Derive album membership from parent directory.
        if let Some(parent) = src.parent() {
            let parent_canonical = canonicalize_path(parent, image_root);
            let parent_str = parent_canonical.to_string_lossy().into_owned();
            if let Some(album_id) = asset_store::get_asset_id_by_path(&parent_str)? {
                record.album_id = Some(album_id);
            }
        }

        asset_store::insert_asset(&record)?;
        record
    };

    info!(
        "Indexed asset {} at {} (hash: {})",
        record.asset_id,
        canonical_str,
        record.content_hash.map_or("none".into(), |h| h.to_string())
    );

    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::TREE;
    use crate::tests::bootstrap::*;
    use redb::ReadableTable;

    fn ensure_asset_tables() {
        let _ = &*TEST_ENV;
        let txn = TREE
            .in_disk
            .begin_write()
            .expect("begin write for table creation");
        txn.open_table(crate::storage::db::ASSET_BY_PATH)
            .expect("create ASSET_BY_PATH");
        txn.open_table(crate::storage::db::ASSET_BY_ID)
            .expect("create ASSET_BY_ID");
        txn.open_table(crate::storage::db::DUPE_INDEX)
            .expect("create DUPE_INDEX");
        txn.commit().expect("commit table creation");
    }

    fn clear_asset_tables() {
        for table_def in [
            crate::storage::db::ASSET_BY_PATH,
            crate::storage::db::ASSET_BY_ID,
            crate::storage::db::DUPE_INDEX,
        ] {
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
    }

    #[test]
    fn index_identical_bytes_at_two_paths_creates_two_assets() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        ensure_asset_tables();

        let image_home = test_image_home();
        let dir = image_home.join("idx_dup");
        std::fs::create_dir_all(&dir).unwrap();

        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(dir.join("a.jpg").to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .unwrap();
        std::fs::copy(dir.join("a.jpg"), dir.join("b.jpg")).unwrap();

        let record_a = index_asset(&dir.join("a.jpg"), &image_home).unwrap();
        let record_b = index_asset(&dir.join("b.jpg"), &image_home).unwrap();

        assert_ne!(record_a.asset_id, record_b.asset_id);
        assert_eq!(
            record_a.content_hash, record_b.content_hash,
            "same bytes → same hash"
        );

        // Both in the same DUPE_INDEX group.
        let hash = record_a.content_hash.unwrap();
        let ids = asset_store::get_dupe_ids(&hash).unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&record_a.asset_id));
        assert!(ids.contains(&record_b.asset_id));

        clear_asset_tables();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn index_same_path_again_is_idempotent() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        ensure_asset_tables();

        let image_home = test_image_home();
        let dir = image_home.join("idx_idem");
        std::fs::create_dir_all(&dir).unwrap();

        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(dir.join("photo.jpg").to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .unwrap();

        let r1 = index_asset(&dir.join("photo.jpg"), &image_home).unwrap();
        let r2 = index_asset(&dir.join("photo.jpg"), &image_home).unwrap();

        assert_eq!(r1.asset_id, r2.asset_id, "same path → same asset ID");
        assert_eq!(r1.content_hash, r2.content_hash);

        // DUPE_INDEX group has exactly 1 entry.
        let hash = r1.content_hash.unwrap();
        let ids = asset_store::get_dupe_ids(&hash).unwrap();
        assert_eq!(ids.len(), 1);

        clear_asset_tables();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn index_changed_bytes_updates_hash_group() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        ensure_asset_tables();

        let image_home = test_image_home();
        let dir = image_home.join("idx_changed");
        std::fs::create_dir_all(&dir).unwrap();

        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(dir.join("photo.jpg").to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .unwrap();

        let r1 = index_asset(&dir.join("photo.jpg"), &image_home).unwrap();
        let old_hash = r1.content_hash.unwrap();

        // Replace the file with different bytes.
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(dir.join("photo.jpg").to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(8),
            height: Some(8),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .unwrap();

        let r2 = index_asset(&dir.join("photo.jpg"), &image_home).unwrap();

        assert_eq!(r1.asset_id, r2.asset_id, "same path → same asset ID");
        assert_ne!(
            old_hash,
            r2.content_hash.unwrap(),
            "different bytes → different hash"
        );

        // Old hash group is empty.
        assert!(asset_store::get_dupe_ids(&old_hash).unwrap().is_empty());
        // New hash group has 1 entry.
        let new_ids = asset_store::get_dupe_ids(&r2.content_hash.unwrap()).unwrap();
        assert_eq!(new_ids.len(), 1);

        clear_asset_tables();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn index_delete_one_path_does_not_affect_other() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        ensure_asset_tables();

        let image_home = test_image_home();
        let dir = image_home.join("idx_delete");
        std::fs::create_dir_all(&dir).unwrap();

        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(dir.join("a.jpg").to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .unwrap();
        std::fs::copy(dir.join("a.jpg"), dir.join("b.jpg")).unwrap();

        let r_a = index_asset(&dir.join("a.jpg"), &image_home).unwrap();
        let r_b = index_asset(&dir.join("b.jpg"), &image_home).unwrap();

        // Remove asset A from the stores.
        asset_store::remove_asset(&r_a).unwrap();

        // Asset B is unaffected.
        let found_b = asset_store::get_asset_by_id(&r_b.asset_id.to_string()).unwrap();
        assert!(found_b.is_some(), "asset B must survive removal of asset A");

        // DUPE_INDEX still has B.
        let hash = r_b.content_hash.unwrap();
        let ids = asset_store::get_dupe_ids(&hash).unwrap();
        assert_eq!(ids.len(), 1);
        assert!(ids.contains(&r_b.asset_id));

        clear_asset_tables();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
