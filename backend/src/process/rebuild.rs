use anyhow::{Context, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::model::asset::{AssetKind, AssetRecord, canonicalize_path};
use crate::model::media::{MediaOutcome, classify_media_file};
use crate::process::hash::blake3_hasher;
use crate::storage::asset_store;

/// One media asset whose metadata pipeline failed during a rebuild.
///
/// The rebuild continues past the failure, so this is the only record of *why*
/// an asset came out of the rebuild without metadata.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RebuildFailure {
    /// Absolute path of the file the pipeline could not process.
    pub path: String,
    /// Rendered error, including the stage that failed.
    pub error: String,
}

/// Upper bound on [`RebuildStats::metadata_failures`].
///
/// A rebuild over a library where every file fails must not produce a response
/// body proportional to the library. [`RebuildStats::metadata_failed`] is
/// therefore the authoritative count and is never capped;
/// [`RebuildStats::metadata_failures_truncated`] says whether details were
/// clipped, and the rest is logged.
pub const MAX_REBUILD_FAILURE_DETAILS: usize = 100;

/// Per-file metadata-pipeline failure accounting for one rebuild.
///
/// A file the pipeline cannot process is a fact about that file, not about the
/// rebuild, so the walk records it and continues — the same recoverable shape a
/// hash error already takes. The record is bounded (see
/// [`MAX_REBUILD_FAILURE_DETAILS`]) but the count is not, and the two are
/// reported separately so a clipped list is never read as a complete one.
#[derive(Debug, Default)]
struct FailureLog {
    failed: usize,
    details: Vec<RebuildFailure>,
    truncated: bool,
}

impl FailureLog {
    fn record(&mut self, path: &Path, error: &anyhow::Error) {
        self.failed += 1;
        if self.details.len() < MAX_REBUILD_FAILURE_DETAILS {
            self.details.push(RebuildFailure {
                path: path.to_string_lossy().into_owned(),
                error: format!("{error:#}"),
            });
        } else {
            if !self.truncated {
                warn!(
                    "rebuild: more than {MAX_REBUILD_FAILURE_DETAILS} files failed the \
                     metadata pipeline; per-file details stop here ({} failures so far)",
                    self.failed
                );
            }
            self.truncated = true;
        }
    }

    fn into_stats(self) -> (usize, Vec<RebuildFailure>, bool) {
        (self.failed, self.details, self.truncated)
    }
}

/// Statistics from a clean filesystem rebuild.
#[derive(Debug, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RebuildStats {
    pub albums_created: usize,
    pub media_created: usize,
    pub unsupported_skipped: usize,
    pub hash_errors: usize,
    /// Media assets whose metadata pipeline returned `Ok` and whose payload was
    /// written to `METADATA_TABLE`. Read against `mediaCreated`, this is what
    /// distinguishes a rebuild that produced usable metadata from one that only
    /// reissued identity.
    pub metadata_indexed: usize,
    /// Media assets whose metadata pipeline failed, counted whether or not the
    /// detail for each one is still in `metadataFailures`.
    pub metadata_failed: usize,
    /// Per-file diagnostics, capped so a wholesale failure cannot size the
    /// response to the library. `metadataFailed` is the authoritative count and
    /// `metadataFailuresTruncated` says whether this list was clipped.
    pub metadata_failures: Vec<RebuildFailure>,
    /// Whether `metadataFailures` stopped short of `metadataFailed`.
    pub metadata_failures_truncated: bool,
}

/// Perform a clean filesystem rebuild starting from empty asset tables.
///
/// Walks `image_root` recursively, creating one `AssetRecord` per physical
/// directory (as an album) and one per valid media file, computing content
/// hashes and populating `DUPE_INDEX` without merging records. Then, for every
/// discovered media file, runs the same metadata pipeline the incremental
/// indexer runs ([`crate::workflow::index_media_file`]) and stores the result in
/// `METADATA_TABLE`.
///
/// `METADATA_TABLE` is cleared with the identity tables, and it has to be:
/// rebuild reissues a random `asset_id` for every asset, so a row keyed by a
/// previous id is unreachable through the API but would otherwise sit in the
/// table indefinitely. Clearing it also means a rebuild is a genuine
/// re-derivation rather than a merge — a payload that a previous index wrote is
/// never the reason a rebuilt asset has metadata.
///
/// A file the metadata pipeline rejects costs only itself: it is counted in
/// [`RebuildStats::metadata_failed`] with its path and the reason, and the walk
/// continues. The rebuild is therefore idempotent for observable metadata —
/// identity reissued and metadata re-derived from the same filesystem state
/// yields the same result — but it does not silently claim success while
/// metadata is missing, because `metadata_indexed` and `metadata_failed` say so
/// in the response.
pub fn rebuild_from_filesystem(image_root: &Path) -> Result<RebuildStats> {
    if !image_root.is_dir() {
        anyhow::bail!(
            "Image root does not exist or is not a directory: {}",
            image_root.display()
        );
    }

    // Clear the identity/duplicate tables *and* the metadata cache. The
    // metadata rows are keyed by the asset ids this run is about to replace.
    clear_asset_tables()?;
    clear_metadata_table()?;

    let mut stats = RebuildStats::default();
    let mut failures = FailureLog::default();

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
            if create_album_asset(path, image_root)? {
                stats.albums_created += 1;
            }
        } else if entry.file_type().is_file() {
            if let MediaOutcome::Skip(reason) = classify_media_file(path) {
                info!("Ignoring unrecognized file {}: {reason:?}", path.display());
                stats.unsupported_skipped += 1;
                continue;
            }

            let outcome = create_media_asset(path, image_root)?;
            stats.hash_errors += usize::from(outcome.hash_failed);
            stats.media_created += 1;

            // The identity record now exists, so the shared metadata pipeline can
            // run over the file. Identity and metadata are deliberately separate
            // steps: the walk must stay an accurate picture of the filesystem
            // even when one file's bytes cannot be read, and
            // `metadata_indexed`/`metadata_failed` are what keep the resulting
            // gap visible rather than silent.
            match outcome.metadata_result {
                Ok(()) => stats.metadata_indexed += 1,
                Err(err) => {
                    warn!(
                        "Rebuild could not derive metadata for {}: {err:#}",
                        path.display()
                    );
                    failures.record(path, &err);
                }
            }
        }
    }

    let (metadata_failed, metadata_failures, metadata_failures_truncated) = failures.into_stats();
    stats.metadata_failed = metadata_failed;
    stats.metadata_failures = metadata_failures;
    stats.metadata_failures_truncated = metadata_failures_truncated;

    info!(
        "Rebuild complete: {} albums, {} media ({} metadata indexed, {} failed), \
         {} unsupported skipped, {} hash errors",
        stats.albums_created,
        stats.media_created,
        stats.metadata_indexed,
        stats.metadata_failed,
        stats.unsupported_skipped,
        stats.hash_errors
    );

    Ok(stats)
}

/// Create the album asset for one directory, returning `false` when the
/// directory already has one (which cannot happen after the tables are cleared,
/// but the walk visits the image root itself and symlink targets can repeat).
fn create_album_asset(path: &Path, image_root: &Path) -> Result<bool> {
    let canonical = canonicalize_path(path, image_root);
    let canonical_str = canonical.to_string_lossy().into_owned();

    if asset_store::get_asset_id_by_path(&canonical_str)?.is_some() {
        return Ok(false);
    }

    let record = AssetRecord::new_album(canonical_str);
    asset_store::insert_asset(&record)
        .with_context(|| format!("Failed to insert album asset for {}", path.display()))?;

    // An album's payload carries only title/cover/count fields, which a
    // directory walk does not carry. The row exists so a rebuilt album composes
    // the same way an indexed one does; the directory album task fills the rest
    // on its next pass.
    store_album_metadata(&record)?;

    Ok(true)
}

/// What one media file contributed to a rebuild: whether its content hash could
/// be read, and the outcome of running the metadata pipeline over it.
struct RebuiltMedia {
    hash_failed: bool,
    metadata_result: Result<()>,
}

/// Create the identity record for one media file, then run the shared metadata
/// pipeline over it.
///
/// Both outcomes are returned rather than propagated: a file whose content hash
/// cannot be read, or whose metadata pipeline fails, still gets an accurate
/// identity record, and the caller turns the failure into a diagnostic.
fn create_media_asset(path: &Path, image_root: &Path) -> Result<RebuiltMedia> {
    let canonical = canonicalize_path(path, image_root);
    let canonical_str = canonical.to_string_lossy().into_owned();

    let kind = if is_image_extension(path) {
        AssetKind::Image
    } else {
        AssetKind::Video
    };

    let file = fs::File::open(path)
        .with_context(|| format!("Failed to open media file {}", path.display()))?;
    let (hash, hash_failed) = match blake3_hasher(file) {
        Ok(h) => (Some(h), false),
        Err(e) => {
            warn!("Failed to hash {}: {e}", path.display());
            (None, true)
        }
    };

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

    let mut record = AssetRecord::new_media(kind, canonical_str, hash, file_size, ext, modified);

    // Derive album membership from parent directory. The walk is depth-first and
    // visits a directory before its entries, so the parent album is already
    // registered.
    if let Some(parent) = path.parent() {
        let parent_canonical = canonicalize_path(parent, image_root);
        let parent_str = parent_canonical.to_string_lossy().into_owned();
        if let Some(album_id) = asset_store::get_asset_id_by_path(&parent_str)? {
            record.album_id = Some(album_id);
        }
    }

    asset_store::insert_asset(&record)
        .with_context(|| format!("Failed to insert media asset for {}", path.display()))?;

    Ok(RebuiltMedia {
        hash_failed,
        metadata_result: index_media_metadata(path, &record),
    })
}

/// Run the shared metadata pipeline for one rebuilt media asset and store the
/// payload under the identity record's own `asset_id`.
///
/// The identity record is written by the walk and is not touched here: this only
/// fills the metadata half, through the same
/// [`crate::process::transitor::store_metadata_record`] the edit and index write
/// paths use, so a rebuilt payload is stored the same way an indexed one is.
fn index_media_metadata(path: &Path, record: &AssetRecord) -> Result<()> {
    let Some(hash) = record.content_hash else {
        // A file whose content hash could not be read has no identity to
        // process against, and inventing one would put it in a false duplicate
        // group. Report it as a failure rather than skip it silently.
        anyhow::bail!("content hash unavailable, so there is nothing to process");
    };

    let data = crate::workflow::index_media_file(path, hash, record.album_id)?;

    crate::process::transitor::store_metadata_record(record.asset_id.as_str(), &data, None)
}

/// Store the metadata payload for a rebuilt album.
///
/// Identity lives on the `AssetRecord`; this is the metadata-only half, derived
/// the same way [`crate::process::transitor::asset_record_to_abstract_data`]
/// projects an album onto the wire.
fn store_album_metadata(record: &AssetRecord) -> Result<()> {
    let data = crate::process::transitor::asset_record_to_abstract_data(record);
    crate::process::transitor::store_metadata_record(record.asset_id.as_str(), &data, None)
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

/// Clear the metadata table.
///
/// Separate from [`clear_one_table`] because `METADATA_TABLE` is typed
/// `<&str, MetadataRecord>` while the three identity/duplicate tables are
/// `<&str, &str>`.
fn clear_metadata_table() -> Result<()> {
    use crate::storage::db::{METADATA_TABLE, TREE};
    use redb::ReadableTable;

    let txn = TREE
        .in_disk
        .begin_write()
        .context("begin write for metadata table clear")?;
    {
        let table = txn
            .open_table(METADATA_TABLE)
            .context("open metadata table for clear")?;
        let keys: Vec<String> = table
            .iter()
            .context("iterate metadata table")?
            .filter_map(|r| r.ok().map(|(k, _)| k.value().to_string()))
            .collect();
        drop(table);
        let mut table = txn
            .open_table(METADATA_TABLE)
            .context("reopen metadata table for clear")?;
        for key in &keys {
            table.remove(key.as_str()).context("remove metadata key")?;
        }
    }
    txn.commit().context("commit metadata table clear")?;
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
            crate::process::format::kind_for_extension(&ext.to_ascii_lowercase())
                == Some(crate::process::format::MediaKind::Image)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::TREE;
    use crate::tests::bootstrap::*;
    use std::path::PathBuf;

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
                further_iptc: None,
                minimal: false,
            },
            snapfab::PhotoSpec {
                output: Some(album_dir.join("sunset.jpg").to_string_lossy().into()),
                format: Some("jpeg".into()),
                width: Some(4),
                height: Some(4),
                tags: None,
                exif_date: None,
                further_iptc: None,
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
            further_iptc: None,
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
            further_iptc: None,
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
            further_iptc: None,
            minimal: false,
        }])
        .unwrap();

        // Create a sidecar file.
        let sidecar_path = album_dir.join("photo.xmp");
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
            further_iptc: None,
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

    // ── Metadata pipeline ──

    fn clear_all_tables() {
        clear_asset_tables().expect("clear asset tables");
        clear_metadata_table().expect("clear METADATA_TABLE");
    }

    /// Every `METADATA_TABLE` key, as owned strings.
    fn metadata_table_keys() -> Vec<String> {
        use crate::storage::db::{METADATA_TABLE, TREE};
        use redb::{ReadableDatabase, ReadableTable};

        let txn = TREE
            .in_disk
            .begin_read()
            .expect("begin read METADATA_TABLE");
        let table = txn.open_table(METADATA_TABLE).expect("open METADATA_TABLE");
        table
            .iter()
            .expect("iterate METADATA_TABLE")
            .filter_map(|row| row.ok().map(|(k, _)| k.value().to_string()))
            .collect()
    }

    /// A directory under the shared `image_home`, removed when the test ends
    /// whether it passed or not.
    ///
    /// Every rebuild test walks the whole of `image_home` and asserts on counts,
    /// so the tests are also asserting that the one before it cleaned up. A
    /// panic that skipped the cleanup would fail the next dozen of them with a
    /// count mismatch, hiding the failure that actually happened.
    struct AlbumDir(PathBuf);

    impl AlbumDir {
        fn under_image_home(name: &str) -> Self {
            let path = test_image_home().join(name);
            fs::create_dir_all(&path).unwrap_or_else(|e| panic!("create {}: {e}", path.display()));
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for AlbumDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn make_jpeg(dir: &Path, name: &str) -> PathBuf {
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(dir.join(name).to_string_lossy().into()),
            format: Some("jpeg".into()),
            // Above snapfab's `minimal` threshold (width and height both <= 4
            // switch to a fixed 2x2 renderer), so the requested size is the
            // size the file actually decodes to. A test that asserts the stored
            // dimensions against the spec needs a spec the spec controls.
            width: Some(8),
            height: Some(8),
            tags: None,
            exif_date: None,
            further_iptc: None,
            minimal: false,
        }])
        .expect("generate jpeg");
        dir.join(name)
    }

    /// A JPEG whose header identifies it as one but whose bytes stop before the
    /// scan completes. `classify_media_file` accepts it (the magic bytes name a
    /// supported format), so it reaches the metadata pipeline and is rejected
    /// there — which is the per-file failure this test needs.
    fn make_truncated_jpeg(dir: &Path, name: &str) -> PathBuf {
        let path = make_jpeg(dir, name);
        let bytes = fs::read(&path).expect("read generated jpeg");
        fs::write(&path, &bytes[..100]).expect("truncate jpeg");
        path
    }

    /// Rebuild must run the metadata pipeline, not just reissue identity: the
    /// stored payload for a rebuilt image carries the dimensions the pipeline
    /// decoded and the EXIF map it read, not the defaults a payload-less
    /// record composes to.
    #[test]
    fn rebuild_writes_metadata_derived_from_the_file() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        ensure_asset_tables();
        clear_all_tables();

        let image_home = test_image_home();
        let album_dir = image_home.join("rebuild_metadata");
        fs::create_dir_all(&album_dir).unwrap();
        let photo = make_jpeg(&album_dir, "photo.jpg");

        let stats = rebuild_from_filesystem(&image_home).expect("rebuild");
        assert_eq!(stats.media_created, 1);
        assert_eq!(stats.metadata_indexed, 1, "one file ran the pipeline");
        assert_eq!(stats.metadata_failed, 0);

        let asset_id = asset_store::get_asset_id_by_path(&photo.to_string_lossy())
            .expect("lookup path")
            .expect("rebuilt asset exists");
        let payload = crate::process::transitor::load_metadata_record(asset_id.as_str())
            .expect("read stored metadata")
            .expect("rebuild must store a metadata row for the rebuilt asset");

        let data = crate::model::metadata_record::compose_abstract_data(
            &asset_store::get_asset_by_id(asset_id.as_str())
                .expect("read record")
                .expect("record exists"),
            Some(&payload),
        );
        assert_eq!(data.width(), 8, "width must come from the decoded file");
        assert_eq!(data.height(), 8, "height must come from the decoded file");
        assert!(
            data.exif_vec().is_some_and(|exif| !exif.is_empty()),
            "rebuild must store the EXIF map the pipeline read"
        );
        assert!(
            data.hash().as_str().contains(|c: char| c != '0'),
            "rebuild must store the content hash the walk computed"
        );

        clear_all_tables();
        fs::remove_dir_all(&album_dir).unwrap();
    }

    /// A file the pipeline rejects must cost only itself. The healthy file beside
    /// it is still processed, the identity walk still covers both, and the
    /// failure is reported with the path that caused it — the same recoverable
    /// style a hash error already uses, since an unprocessable file is a fact
    /// about one file and not about the rebuild.
    #[test]
    fn a_pipeline_failure_costs_only_its_own_file_and_is_reported() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        ensure_asset_tables();
        clear_all_tables();

        let image_home = test_image_home();
        let album_dir = image_home.join("rebuild_partial_failure");
        fs::create_dir_all(&album_dir).unwrap();
        let good = make_jpeg(&album_dir, "good.jpg");
        let broken = make_truncated_jpeg(&album_dir, "broken.jpg");

        let stats = rebuild_from_filesystem(&image_home).expect("rebuild must not abort");

        assert_eq!(
            stats.media_created, 2,
            "identity is issued by the walk, independently of the pipeline"
        );
        assert_eq!(stats.metadata_indexed, 1, "only the healthy file processed");
        assert_eq!(stats.metadata_failed, 1);
        assert!(!stats.metadata_failures_truncated);

        assert_eq!(
            stats.metadata_failures.len(),
            1,
            "one failure, one diagnostic: {stats:?}"
        );
        let failure = &stats.metadata_failures[0];
        assert_eq!(
            failure.path,
            broken.to_string_lossy(),
            "the diagnostic must name the file that could not be processed"
        );
        assert!(
            !failure.error.is_empty(),
            "a diagnostic without the reason is not actionable"
        );

        // The healthy file is fully processed despite its neighbour's failure.
        let good_id = asset_store::get_asset_id_by_path(&good.to_string_lossy())
            .expect("lookup path")
            .expect("good.jpg was indexed");
        let good_payload = crate::process::transitor::load_metadata_record(good_id.as_str())
            .expect("read stored metadata")
            .expect("good.jpg must have a metadata row");

        // The failed file keeps its identity record but has no metadata row.
        let broken_id = asset_store::get_asset_id_by_path(&broken.to_string_lossy())
            .expect("lookup path")
            .expect("broken.jpg was indexed");
        assert!(
            crate::process::transitor::load_metadata_record(broken_id.as_str())
                .expect("read stored metadata")
                .is_none(),
            "a file the pipeline rejected must not leave a default payload behind"
        );

        assert_eq!(
            good_payload.tags().len(),
            0,
            "an untagged fixture has no tags; the row is a real payload, not a stub"
        );

        clear_all_tables();
        fs::remove_dir_all(&album_dir).unwrap();
    }

    /// The detail list is bounded so a library that fails wholesale cannot turn
    /// the response into an unbounded body, but the count is not: a truncated
    /// detail list must be visible in the response rather than only in the log.
    ///
    /// Driven through [`FailureLog`] directly. Building `MAX +
    /// 1` unprocessable files to reach the same state would cost a filesystem
    /// walk and an ExifTool read each, and the accounting is the thing under
    /// test, not the media.
    #[test]
    fn failure_details_are_bounded_but_the_count_is_not() {
        for i in 0..=MAX_REBUILD_FAILURE_DETAILS {
            let mut log = FailureLog::default();
            log.record(
                Path::new(&format!("/photos/broken{i}.jpg")),
                &anyhow::anyhow!("failed to decode image into DynamicImage"),
            );
            // A rebuild that failed once, once for every file, and once past the
            // cap. The count must be exact in all three; only the details differ.
            let (failed, details, truncated) = log.into_stats();
            assert_eq!(failed, 1, "one failure counted after {i} record(s)");
            assert_eq!(details.len(), 1);
            assert_eq!(details[0].path, format!("/photos/broken{i}.jpg"));
            assert!(!truncated);
        }

        // Past the cap: the count keeps rising, the details stop, and the
        // response says so.
        let mut log = FailureLog::default();
        for i in 0..MAX_REBUILD_FAILURE_DETAILS + 7 {
            log.record(
                Path::new(&format!("/photos/broken{i}.jpg")),
                &anyhow::anyhow!("boom {i}"),
            );
        }
        let (failed, details, truncated) = log.into_stats();
        assert_eq!(failed, MAX_REBUILD_FAILURE_DETAILS + 7);
        assert_eq!(details.len(), MAX_REBUILD_FAILURE_DETAILS);
        assert!(
            truncated,
            "a clipped detail list must be reported, not silent"
        );
    }

    /// Rebuild reissues `asset_id`s, so the `METADATA_TABLE` rows keyed by the
    /// previous ids are unreachable by construction. This pins that they are
    /// actually gone rather than merely unreferenced: a row left behind is
    /// invisible through the API but still occupies the table forever, and it
    /// would resurface if an id were ever reused.
    #[test]
    fn rebuild_leaves_no_metadata_row_for_a_superseded_asset_id() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        ensure_asset_tables();
        clear_all_tables();

        let image_home = test_image_home();
        let album_dir = image_home.join("rebuild_stale_rows");
        fs::create_dir_all(&album_dir).unwrap();
        let photo = make_jpeg(&album_dir, "photo.jpg");

        // A first pass, so a populated table with real ids exists.
        let first = rebuild_from_filesystem(&image_home).expect("first rebuild");
        let first_id = asset_store::get_asset_id_by_path(&photo.to_string_lossy())
            .expect("lookup path")
            .expect("asset exists")
            .to_string();
        assert!(
            metadata_table_keys().contains(&first_id),
            "the first rebuild must store a row under its own id"
        );

        let second = rebuild_from_filesystem(&image_home).expect("second rebuild");
        assert_eq!(second.media_created, first.media_created);

        let keys = metadata_table_keys();
        assert!(
            !keys.contains(&first_id),
            "the superseded id {first_id} must not keep a metadata row: {keys:?}"
        );

        // One row per rebuilt asset: the image plus the two album directories
        // (the image home and `rebuild_stale_rows`).
        let assets = asset_store::get_all_assets().expect("read assets");
        assert_eq!(
            keys.len(),
            assets.len(),
            "every rebuilt asset needs a row and no other key may survive"
        );
        for record in &assets {
            assert!(
                keys.contains(&record.asset_id.to_string()),
                "asset {} has no metadata row",
                record.asset_id
            );
        }

        clear_all_tables();
        fs::remove_dir_all(&album_dir).unwrap();
    }

    /// A toolchain failure is not a fact about the file being walked, so it must
    /// not be reported as one. Before the pipeline propagated it, a rebuild
    /// against a deployment without a usable `exiftool` stored a default payload
    /// for every file — `exifVec: {}`, no tags, no dimensions — and answered
    /// `metadataIndexed` as a success. That is the failure this counts instead:
    /// identity is still issued for the file, the pipeline error is recorded as
    /// a metadata failure, and the diagnostic names the remedy, so an operator
    /// reading the rebuild response knows the library was not re-derived.
    #[test]
    fn a_toolchain_failure_is_counted_as_a_metadata_failure_with_the_remedy() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        ensure_asset_tables();
        clear_all_tables();

        let image_home = test_image_home();
        let album_dir = AlbumDir::under_image_home("rebuild_toolchain_failure");
        let photo = make_jpeg(album_dir.path(), "photo.jpg");

        let stats = crate::process::exif::with_read_seam(
            crate::process::exif::ReadSeam::Executable(PathBuf::from(
                "/nonexistent/picasu-no-such-exiftool",
            )),
            || rebuild_from_filesystem(&image_home).expect("the walk itself must not abort"),
        );

        assert_eq!(
            stats.media_created, 1,
            "the identity walk is independent of the pipeline"
        );
        assert_eq!(
            stats.metadata_indexed, 0,
            "nothing was derived, so a rebuild reporting success here is the silent-empty bug"
        );
        assert_eq!(stats.metadata_failed, 1, "the toolchain failure is counted");
        assert!(!stats.metadata_failures_truncated);

        let failure = &stats.metadata_failures[0];
        assert_eq!(failure.path, photo.to_string_lossy());
        for expected in [
            "exiftool",
            "just install-exiftool",
            "apt-get install libimage-exiftool-perl",
        ] {
            assert!(
                failure.error.contains(expected),
                "the per-file diagnostic has to be actionable, and has to mention {expected:?}: \
                 {}",
                failure.error
            );
        }

        let asset_id = asset_store::get_asset_id_by_path(&photo.to_string_lossy())
            .expect("lookup path")
            .expect("the walk issued identity for the file");
        assert!(
            crate::process::transitor::load_metadata_record(asset_id.as_str())
                .expect("read stored metadata")
                .is_none(),
            "a pipeline that never ran must not leave a default payload behind"
        );

        clear_all_tables();
    }

    /// The other side of the same classification: a file whose own metadata
    /// `ExifTool` will not parse costs nothing. The rebuild stores the asset
    /// with the empty `exifVec` it derived and reports it as indexed, because
    /// that is the truth about the file rather than a gap in the rebuild.
    #[test]
    fn a_file_exiftool_rejects_is_rebuilt_with_empty_metadata() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        ensure_asset_tables();
        clear_all_tables();

        let image_home = test_image_home();
        let album_dir = AlbumDir::under_image_home("rebuild_soft_metadata_failure");
        let good = make_jpeg(album_dir.path(), "good.jpg");
        // A decodable image whose EXIF block cannot be parsed. A read error is
        // injected for the file itself, because ExifTool 13.59 answers a
        // malformed EXIF segment with a *successful* read of a record with no
        // EXIF groups rather than with an error.
        let mut bytes = fs::read(&good).expect("read the generated jpeg");
        crate::process::exif::corrupt_exif_byte_order(&mut bytes);
        let damaged = album_dir.path().join("damaged.jpg");
        fs::write(&damaged, &bytes).expect("write the damaged jpeg");

        let stats = crate::process::exif::with_read_seam(
            crate::process::exif::ReadSeam::Injected(Err(
                exiftool::ExifToolError::ExifToolProcess {
                    message: "Error: Malformed APP1 EXIF segment".to_string(),
                    std_err: "Error: Malformed APP1 EXIF segment".to_string(),
                    command_args: "-json -G1 damaged.jpg".to_string(),
                },
            )),
            || rebuild_from_filesystem(&image_home).expect("rebuild"),
        );

        assert_eq!(
            stats.metadata_failed, 0,
            "a bad file is not a rebuild failure"
        );
        assert_eq!(
            stats.metadata_indexed, 2,
            "both files are assets worth having, with whatever the file yielded"
        );

        let asset_id = asset_store::get_asset_id_by_path(&damaged.to_string_lossy())
            .expect("lookup path")
            .expect("the damaged file was indexed");
        let payload = crate::process::transitor::load_metadata_record(asset_id.as_str())
            .expect("read stored metadata")
            .expect("the damaged file has a real payload, not a missing one");
        let crate::model::metadata_record::MetadataRecord::Image(payload) = &payload else {
            panic!("a rebuilt .jpg is an image payload")
        };
        assert_eq!(
            payload.exif_vec.len(),
            0,
            "and its exifVec is the empty map the failed read produced"
        );
        assert_eq!(
            (payload.width, payload.height),
            (8, 8),
            "while the rest of the pipeline still decoded the image"
        );

        clear_all_tables();
    }
}
