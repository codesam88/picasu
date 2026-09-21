#![allow(dead_code)]

use anyhow::{Context, Result};
use log::{info, warn};
use std::fs;
use std::path::Path;

use crate::model::asset::{AssetKind, AssetRecord, canonicalize_path};
use crate::model::media::is_valid_media_file;
use crate::process::hash::blake3_hasher;
use crate::storage::asset_store;

/// Statistics from a clean filesystem rebuild.
#[derive(Debug, Default)]
pub struct RebuildStats {
    pub albums_created: usize,
    pub media_created: usize,
    pub unsupported_skipped: usize,
    pub hash_errors: usize,
}

/// Perform a clean filesystem rebuild starting from empty asset tables.
///
/// Walks `image_root` recursively, creating one `AssetRecord` per physical
/// directory (as an album) and one per valid media file.  Computes content
/// hashes for media files and populates `DUPE_INDEX` without merging records.
///
/// The rebuild is idempotent: it clears the asset tables first, then
/// repopulates them from the filesystem.  The filesystem is the source of
/// truth.
pub fn rebuild_from_filesystem(image_root: &Path) -> Result<RebuildStats> {
    if !image_root.is_dir() {
        anyhow::bail!(
            "Image root does not exist or is not a directory: {}",
            image_root.display()
        );
    }

    // Clear the new tables.
    clear_asset_tables()?;

    let mut stats = RebuildStats::default();

    // Walk the filesystem depth-first.
    let walker = walkdir::WalkDir::new(image_root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            // Skip internal data directories.
            !is_internal_entry(entry.path())
        });

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                warn!("Rebuild walk error: {err}");
                continue;
            }
        };

        let path = entry.path();

        if entry.file_type().is_dir() {
            // Create an album asset for every directory under image_root,
            // including the root itself.
            let canonical = canonicalize_path(path, image_root);
            let canonical_str = canonical.to_string_lossy().into_owned();

            // Skip if already exists (shouldn't after clear, but be safe).
            if asset_store::get_asset_id_by_path(&canonical_str)?.is_some() {
                continue;
            }

            let record = AssetRecord::new_album(canonical_str);
            asset_store::insert_asset(&record)
                .with_context(|| format!("Failed to insert album asset for {}", path.display()))?;
            stats.albums_created += 1;
        } else if entry.file_type().is_file() {
            if !is_valid_media_file(path) {
                stats.unsupported_skipped += 1;
                continue;
            }

            let canonical = canonicalize_path(path, image_root);
            let canonical_str = canonical.to_string_lossy().into_owned();

            let kind = if is_image_extension(path) {
                AssetKind::Image
            } else {
                AssetKind::Video
            };

            // Compute content hash.
            let file = fs::File::open(path)
                .with_context(|| format!("Failed to open media file {}", path.display()))?;
            let hash = match blake3_hasher(file) {
                Ok(h) => Some(h),
                Err(e) => {
                    warn!("Failed to hash {}: {e}", path.display());
                    stats.hash_errors += 1;
                    None
                }
            };

            // Get file metadata.
            let md = fs::metadata(path)
                .with_context(|| format!("Failed to read metadata for {}", path.display()))?;
            let file_size = md.len();
            let modified = md.modified().map_or(0, |t| {
                let millis = t
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                i64::try_from(millis).unwrap_or(0)
            });

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();

            let mut record =
                AssetRecord::new_media(kind, canonical_str, hash, file_size, ext, modified);

            // Derive album membership from parent directory.
            if let Some(parent) = path.parent() {
                let parent_canonical = canonicalize_path(parent, image_root);
                let parent_str = parent_canonical.to_string_lossy().into_owned();
                if let Some(album_id) = asset_store::get_asset_id_by_path(&parent_str)? {
                    record.album_id = Some(album_id);
                }
            }

            asset_store::insert_asset(&record)
                .with_context(|| format!("Failed to insert media asset for {}", path.display()))?;
            stats.media_created += 1;
        }
    }

    info!(
        "Rebuild complete: {} albums, {} media, {} unsupported skipped, {} hash errors",
        stats.albums_created, stats.media_created, stats.unsupported_skipped, stats.hash_errors
    );

    Ok(stats)
}

/// Clear all three asset tables.
fn clear_asset_tables() -> Result<()> {
    clear_one_table(crate::storage::db::ASSET_BY_PATH)?;
    clear_one_table(crate::storage::db::ASSET_BY_ID)?;
    clear_one_table(crate::storage::db::DUPE_INDEX)?;
    Ok(())
}

fn clear_one_table(
    table_def: redb::TableDefinition<'static, &'static str, &'static str>,
) -> Result<()> {
    use crate::storage::db::TREE;
    use redb::ReadableTable;

    let txn = TREE
        .in_disk
        .begin_write()
        .context("begin write for table clear")?;
    {
        let table = txn.open_table(table_def).context("open table for clear")?;
        let keys: Vec<String> = table
            .iter()
            .context("iterate table")?
            .filter_map(|r| r.ok().map(|(k, _)| k.value().to_string()))
            .collect();
        drop(table);
        let mut table = txn
            .open_table(table_def)
            .context("reopen table for clear")?;
        for key in &keys {
            table.remove(key.as_str()).context("remove key")?;
        }
    }
    txn.commit().context("commit table clear")?;
    Ok(())
}

/// Returns `true` if `path` is inside an internal Picasu data directory
/// (e.g., `object/`, `db/`).
fn is_internal_entry(path: &Path) -> bool {
    // Check each component of the path for internal directory names.
    path.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        s == "db" || s == "object"
    })
}

fn is_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            crate::constant::VALID_IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::TREE;
    use crate::tests::bootstrap::*;

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

    #[test]
    fn rebuild_creates_one_asset_per_file_and_directory() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        ensure_asset_tables();

        let image_home = test_image_home();

        // Create a small filesystem tree.
        let album_dir = image_home.join("vacation");
        fs::create_dir_all(&album_dir).unwrap();
        snapfab::generate_batch(&[
            snapfab::PhotoSpec {
                output: Some(album_dir.join("beach.jpg").to_string_lossy().into()),
                format: Some("jpeg".into()),
                width: Some(4),
                height: Some(4),
                tags: None,
                exif_date: None,
                minimal: false,
            },
            snapfab::PhotoSpec {
                output: Some(album_dir.join("sunset.jpg").to_string_lossy().into()),
                format: Some("jpeg".into()),
                width: Some(4),
                height: Some(4),
                tags: None,
                exif_date: None,
                minimal: false,
            },
        ])
        .expect("generate photos");

        let stats = rebuild_from_filesystem(&image_home).expect("rebuild");

        // Albums: image_root + vacation = 2
        assert_eq!(stats.albums_created, 2, "expected 2 album assets");
        // Media: beach.jpg + sunset.jpg = 2
        assert_eq!(stats.media_created, 2, "expected 2 media assets");
        assert_eq!(stats.hash_errors, 0);

        // Verify each file has a unique asset ID.
        let beach_id =
            asset_store::get_asset_id_by_path(&album_dir.join("beach.jpg").to_string_lossy())
                .unwrap();
        let sunset_id =
            asset_store::get_asset_id_by_path(&album_dir.join("sunset.jpg").to_string_lossy())
                .unwrap();
        assert!(beach_id.is_some(), "beach.jpg must have an asset");
        assert!(sunset_id.is_some(), "sunset.jpg must have an asset");
        assert_ne!(
            beach_id, sunset_id,
            "different files must have different asset IDs"
        );

        // Verify album asset exists.
        let album_id = asset_store::get_asset_id_by_path(&album_dir.to_string_lossy()).unwrap();
        assert!(album_id.is_some(), "album directory must have an asset");

        // Verify media assets have content hashes in DUPE_INDEX.
        let beach_record = asset_store::get_asset_by_id(&beach_id.unwrap().to_string())
            .unwrap()
            .unwrap();
        assert!(
            beach_record.content_hash.is_some(),
            "media must have content hash"
        );

        // Cleanup.
        clear_asset_tables().unwrap();
        fs::remove_dir_all(&album_dir).unwrap();
    }

    #[test]
    fn rebuild_duplicate_files_get_separate_assets() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        ensure_asset_tables();

        let image_home = test_image_home();
        let album_dir = image_home.join("dup_test");
        fs::create_dir_all(&album_dir).unwrap();

        // Create one photo and copy it.
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(album_dir.join("original.jpg").to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .expect("generate photo");

        fs::copy(album_dir.join("original.jpg"), album_dir.join("copy.jpg")).expect("copy file");

        let stats = rebuild_from_filesystem(&image_home).expect("rebuild");

        assert_eq!(stats.media_created, 2, "two files → two media assets");

        // Both files have separate asset IDs.
        let orig_id =
            asset_store::get_asset_id_by_path(&album_dir.join("original.jpg").to_string_lossy())
                .unwrap();
        let copy_id =
            asset_store::get_asset_id_by_path(&album_dir.join("copy.jpg").to_string_lossy())
                .unwrap();
        assert_ne!(
            orig_id, copy_id,
            "duplicate files must have different asset IDs"
        );

        // Both are in the same DUPE_INDEX group.
        let orig_record = asset_store::get_asset_by_id(&orig_id.unwrap().to_string())
            .unwrap()
            .unwrap();
        let hash = orig_record.content_hash.unwrap();
        let dupe_ids = asset_store::get_dupe_ids(&hash).unwrap();
        assert_eq!(dupe_ids.len(), 2, "dupe group must contain both assets");
        assert!(dupe_ids.contains(&orig_id.unwrap()));
        assert!(dupe_ids.contains(&copy_id.unwrap()));

        // Cleanup.
        clear_asset_tables().unwrap();
        fs::remove_dir_all(&album_dir).unwrap();
    }

    #[test]
    fn rebuild_unsupported_files_are_skipped() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        ensure_asset_tables();

        let image_home = test_image_home();
        let album_dir = image_home.join("unsupported_test");
        fs::create_dir_all(&album_dir).unwrap();

        // Create a non-media file.
        fs::write(album_dir.join("readme.txt"), "not a photo").unwrap();

        let stats = rebuild_from_filesystem(&image_home).expect("rebuild");

        assert_eq!(stats.unsupported_skipped, 1, "txt file should be skipped");
        assert_eq!(stats.media_created, 0, "no media should be created");

        // Cleanup.
        clear_asset_tables().unwrap();
        fs::remove_dir_all(&album_dir).unwrap();
    }

    #[test]
    fn rebuild_empty_directory_becomes_album() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        ensure_asset_tables();

        let image_home = test_image_home();
        let empty_dir = image_home.join("empty_album");
        fs::create_dir_all(&empty_dir).unwrap();

        let stats = rebuild_from_filesystem(&image_home).expect("rebuild");

        // image_root + empty_album = 2 albums
        assert!(stats.albums_created >= 2);
        assert_eq!(stats.media_created, 0);

        let album_id = asset_store::get_asset_id_by_path(&empty_dir.to_string_lossy()).unwrap();
        assert!(
            album_id.is_some(),
            "empty directory must become an album asset"
        );

        let record = asset_store::get_asset_by_id(&album_id.unwrap().to_string())
            .unwrap()
            .unwrap();
        assert_eq!(record.kind, AssetKind::Album);
        assert!(
            record.content_hash.is_none(),
            "album must not have content hash"
        );

        // Cleanup.
        clear_asset_tables().unwrap();
        fs::remove_dir_all(&empty_dir).unwrap();
    }

    #[test]
    fn rebuild_stale_dupe_index_cleaned_on_rebuild() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        ensure_asset_tables();

        let image_home = test_image_home();
        let album_dir = image_home.join("stale_dupe");
        fs::create_dir_all(&album_dir).unwrap();

        // Create a photo.
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(album_dir.join("photo.jpg").to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .unwrap();

        // First rebuild.
        let stats1 = rebuild_from_filesystem(&image_home).unwrap();
        assert_eq!(stats1.media_created, 1);

        let photo_id =
            asset_store::get_asset_id_by_path(&album_dir.join("photo.jpg").to_string_lossy())
                .unwrap()
                .unwrap();
        let record = asset_store::get_asset_by_id(&photo_id.to_string())
            .unwrap()
            .unwrap();
        let hash = record.content_hash.unwrap();
        let dupe_ids = asset_store::get_dupe_ids(&hash).unwrap();
        assert_eq!(dupe_ids.len(), 1, "dupe group should have 1 entry");

        // Delete the file.
        fs::remove_file(album_dir.join("photo.jpg")).unwrap();

        // Second rebuild — stale dupe entry should be cleaned.
        let stats2 = rebuild_from_filesystem(&image_home).unwrap();
        assert_eq!(stats2.media_created, 0, "no media after file deleted");

        // DUPE_INDEX should be empty for this hash.
        let dupe_ids_after = asset_store::get_dupe_ids(&hash).unwrap();
        assert!(
            dupe_ids_after.is_empty(),
            "stale dupe entry should be removed after rebuild"
        );

        // Cleanup.
        clear_asset_tables().unwrap();
        fs::remove_dir_all(&album_dir).unwrap();
    }

    #[test]
    fn rebuild_preserves_sidecar_files() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        ensure_asset_tables();

        let image_home = test_image_home();
        let album_dir = image_home.join("sidecar_test");
        fs::create_dir_all(&album_dir).unwrap();

        // Create a photo.
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(album_dir.join("photo.jpg").to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .unwrap();

        // Create a sidecar file.
        let sidecar_path = album_dir.join("photo.jpg.xmp");
        fs::write(&sidecar_path, "<x:xmpmeta></x:xmpmeta>").unwrap();

        let stats = rebuild_from_filesystem(&image_home).unwrap();
        assert_eq!(stats.media_created, 1);

        // Sidecar should still exist on disk after rebuild.
        assert!(
            sidecar_path.exists(),
            "sidecar must not be deleted by rebuild"
        );

        // Cleanup.
        clear_asset_tables().unwrap();
        fs::remove_dir_all(&album_dir).unwrap();
    }

    #[test]
    fn rebuild_nested_directories_become_album_assets() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        ensure_asset_tables();

        let image_home = test_image_home();
        let nested = image_home.join("a/b/c");
        fs::create_dir_all(&nested).unwrap();

        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(nested.join("deep.jpg").to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(4),
            height: Some(4),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .unwrap();

        let stats = rebuild_from_filesystem(&image_home).unwrap();

        // image_root + a + a/b + a/b/c = 4 albums
        assert!(stats.albums_created >= 4, "nested dirs must become albums");
        assert_eq!(stats.media_created, 1);

        // Each nested directory has an album asset.
        for dir in ["a", "a/b", "a/b/c"] {
            let album_id =
                asset_store::get_asset_id_by_path(&image_home.join(dir).to_string_lossy()).unwrap();
            assert!(
                album_id.is_some(),
                "directory {dir} must have an album asset"
            );
            let record = asset_store::get_asset_by_id(&album_id.unwrap().to_string())
                .unwrap()
                .unwrap();
            assert_eq!(record.kind, AssetKind::Album);
        }

        // Cleanup.
        clear_asset_tables().unwrap();
        fs::remove_dir_all(&image_home.join("a")).unwrap();
    }
}
