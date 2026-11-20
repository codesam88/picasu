use crate::constant::DEFAULT_PRIORITY_LIST;
use crate::model::abstract_data::AbstractData;
use crate::model::metadata_record::MetadataRecord;
use crate::model::response::DataBaseTimestampReturn;
use crate::storage::cache::MyCow;
use anyhow::{Context, Result};
use arrayvec::ArrayString;

pub fn index_to_asset_id(tree_snapshot: &MyCow, index: usize) -> Result<ArrayString<64>> {
    // `len()` is fallible (redb read failure propagates as an error rather
    // than panicking inside a handler); `?` lifts it into the anyhow error.
    let len = tree_snapshot.len()?;
    if index >= len {
        return Err(anyhow::anyhow!("Index out of bounds: {index}"));
    }
    let asset_id = tree_snapshot.get_asset_id(index)?;
    Ok(asset_id)
}

/// Read the stored metadata-only payload for `asset_id`, if any.
pub fn load_metadata_record(asset_id: &str) -> Result<Option<MetadataRecord>> {
    use crate::storage::db::{METADATA_TABLE, TREE};

    let txn = TREE.in_disk.begin_read()?;
    let table = txn.open_table(METADATA_TABLE)?;
    Ok(table.get(asset_id)?.map(|guard| guard.value()))
}

/// Compose the wire `AbstractData` view for `asset_id` from its identity
/// `AssetRecord` plus optional stored payload. Returns `None` when the asset
/// record does not exist.
pub fn compose_by_asset_id(asset_id: &str) -> Result<Option<AbstractData>> {
    let Some(record) = crate::storage::asset_store::get_asset_by_id(asset_id)? else {
        return Ok(None);
    };
    let payload = load_metadata_record(asset_id)?;
    Ok(Some(crate::model::metadata_record::compose_abstract_data(
        &record,
        payload.as_ref(),
    )))
}

/// Metadata-edit write path: upsert the metadata-only payload extracted
/// from `data` under `asset_id`, in one write transaction.
///
/// Identity is never read out of `data` for writing. When `trash` is
/// `Some(flag)`, `AssetRecord.is_trashed` is read-modify-written inside the
/// same transaction (redb is single-writer, so the `ASSET_BY_ID` JSON row
/// is updated directly here rather than via store helpers that open their
/// own transaction).
pub fn store_metadata_record(
    asset_id: &str,
    data: &AbstractData,
    trash: Option<bool>,
) -> Result<()> {
    use crate::model::asset::AssetRecord;
    use crate::model::metadata_record::to_metadata_record;
    use crate::storage::db::{ASSET_BY_ID, METADATA_TABLE, TREE};
    use redb::ReadableTable;

    use crate::storage::asset_store;

    let payload = to_metadata_record(data);

    // Snapshot the ancestor album IDs before taking the write transaction:
    // `ancestor_album_ids_for_path` locks `DIR_ALBUM_CACHE`, which must never
    // be acquired while holding a write txn (lock-order convention).
    let ancestor_ids = crate::storage::asset_store::get_asset_by_id(asset_id)
        .ok()
        .flatten()
        .map(|r| {
            crate::process::dir_album::ancestor_album_ids_for_path(std::path::Path::new(&r.path))
        })
        .unwrap_or_default();

    let txn = TREE.in_disk.begin_write()?;
    {
        let mut metadata_table = txn.open_table(METADATA_TABLE)?;
        metadata_table.insert(asset_id, payload)?;

        if let Some(is_trashed) = trash {
            let mut id_table = txn.open_table(ASSET_BY_ID)?;
            let existing = id_table
                .get(asset_id)?
                .map(|guard| guard.value().to_string());
            if let Some(json) = existing {
                let mut record: AssetRecord = serde_json::from_str(&json)?;
                let old_trashed = record.is_trashed;
                let album_id = record.album_id;
                let file_size = record.file_size;
                record.is_trashed = is_trashed;
                let updated = serde_json::to_string(&record)?;
                id_table.insert(asset_id, updated.as_str())?;

                // Release both handles before the delta helpers reopen these
                // tables in the same transaction (redb allows one open handle
                // per table per txn).
                drop(id_table);
                drop(metadata_table);

                // Membership change → album aggregate delta + member row in the
                // same transaction. No-op when the asset is unalbummed.
                if old_trashed != is_trashed
                    && let Some(alb) = album_id
                {
                    let member: ArrayString<64> = asset_id.parse().map_err(|_| {
                        anyhow::anyhow!("asset id exceeds 64 bytes for album delta: {asset_id}")
                    })?;
                    let delta = if is_trashed {
                        // Trashed: member leaves.
                        asset_store::AlbumDelta {
                            count_delta: -1,
                            size_delta: -file_size.cast_signed(),
                            member: Some(member),
                            is_add: false,
                            member_timestamp: None,
                            member_size_override: Some(file_size),
                            is_update: false,
                        }
                    } else {
                        // Untrashed: member joins.
                        asset_store::AlbumDelta {
                            count_delta: 1,
                            size_delta: 0,
                            member: Some(member),
                            is_add: true,
                            member_timestamp: None,
                            member_size_override: Some(file_size),
                            is_update: false,
                        }
                    };
                    asset_store::apply_album_delta(&txn, alb.as_str(), delta).with_context(
                        || format!("Failed to apply album delta for album {alb} asset {asset_id}"),
                    )?;
                    for anc in &ancestor_ids {
                        if *anc != alb {
                            asset_store::refresh_empty_album_cover(&txn, anc.as_str())
                                .with_context(|| {
                                    format!("Failed to refresh ancestor album cover {anc}")
                                })?;
                        }
                    }
                }
            }
        }
    }
    txn.commit()?;
    Ok(())
}

/// Convert an `AssetRecord` to a payload-less `AbstractData` for API
/// responses. This is a lossy conversion — EXIF, tags, and other metadata
/// are not preserved.
/// Uses content hash as `object.id` for media items (required for compressed
/// thumbnail path resolution). Uses `asset_id` for albums (no thumbnails).
pub fn asset_record_to_abstract_data(record: &crate::model::asset::AssetRecord) -> AbstractData {
    use crate::model::album::{AlbumCombined, AlbumMetadata};
    use crate::model::asset::AssetKind;
    use crate::model::image::{ImageCombined, ImageMetadata};
    use crate::model::object::{ObjectSchema, ObjectType};
    use crate::model::response::FileEntry;
    use crate::model::video::{VideoCombined, VideoMetadata};

    // Media items use content hash as object.id (required for compressed
    // thumbnail path resolution). Albums use asset_id (no thumbnails).
    let display_id = record.content_hash.unwrap_or(record.asset_id);

    match record.kind {
        AssetKind::Image => {
            let object = ObjectSchema::new(display_id, ObjectType::Image);
            let mut metadata = ImageMetadata::new(record.file_size, 0, 0, record.ext.clone());
            metadata.path = Some(FileEntry {
                file: record.path.clone(),
                modified: record.modified,
                scan_time: record.scan_time,
                is_trashed: record.is_trashed,
            });
            AbstractData::Image(ImageCombined { object, metadata })
        }
        AssetKind::Video => {
            let object = ObjectSchema::new(display_id, ObjectType::Video);
            let mut metadata = VideoMetadata::new(record.file_size, 0, 0, record.ext.clone());
            metadata.path = Some(FileEntry {
                file: record.path.clone(),
                modified: record.modified,
                scan_time: record.scan_time,
                is_trashed: record.is_trashed,
            });
            AbstractData::Video(VideoCombined { object, metadata })
        }
        AssetKind::Album => {
            // Albums use asset_id as display_id (no compressed thumbnails).
            let album_id = record.asset_id;
            let object = ObjectSchema::new(album_id, ObjectType::Album);
            let metadata = AlbumMetadata {
                id: album_id,
                title: None,
                created_time: record.scan_time,
                start_time: None,
                end_time: None,
                last_modified_time: record.modified,
                cover: None,
                manual_cover: None,
                item_count: 0,
                item_size: 0,
                share_list: std::collections::HashMap::new(),
                dir_path: record.path.clone(),
                custom_title: None,
                is_trashed: record.is_trashed,
            };
            AbstractData::Album(AlbumCombined { object, metadata })
        }
    }
}

/// Build the lean list-row `AbstractData` for an image/video from its identity
/// `AssetRecord` plus the snapshot-carried display fields.
///
/// Phase 14 split: `get-data` no longer reads `METADATA_TABLE` per media row.
/// The row carries identity, dimensions, the file entry, album membership, and
/// the cache-bust/processing keys (`update_at`, `pending`) — tags, EXIF, and
/// description are deliberately absent and must be fetched via
/// `GET /get/metadata/{assetId}` (detail/sidebar). Rating likewise lives
/// behind the detail endpoint.
pub fn lean_media_abstract_data(
    record: &crate::model::asset::AssetRecord,
    reduced: &crate::model::response::ReducedData,
) -> AbstractData {
    let mut data = asset_record_to_abstract_data(record);
    data.set_width(reduced.width);
    data.set_height(reduced.height);
    // Album membership comes from the identity record, not the metadata row.
    data.set_album(record.album_id);
    match &mut data {
        AbstractData::Image(img) => {
            img.object.update_at = reduced.update_at;
            img.object.pending = reduced.pending;
        }
        AbstractData::Video(vid) => {
            vid.object.update_at = reduced.update_at;
            vid.object.pending = reduced.pending;
        }
        AbstractData::Album(_) => {}
    }
    data
}

/// Strip share-hidden metadata fields from a response row.
///
/// When `show_metadata` is false (a share that hides metadata), clears the
/// album membership, tags, stored path, and EXIF so the filesystem path cannot
/// leak through the shared view; tile rendering and locate rely on the
/// row-level `asset_id`, not the stored path.
pub fn clear_abstract_data_metadata(abstract_data: &mut AbstractData, show_metadata: bool) {
    match abstract_data {
        AbstractData::Image(img) => {
            if !show_metadata {
                img.metadata.album = None;
                img.object.tags.clear();
                img.metadata.path = None;
                img.metadata.exif_vec.clear();
            }
        }
        AbstractData::Video(vid) => {
            if !show_metadata {
                vid.metadata.album = None;
                vid.object.tags.clear();
                vid.metadata.path = None;
                vid.metadata.exif_vec.clear();
            }
        }
        AbstractData::Album(album) => {
            if !show_metadata {
                album.object.tags.clear();
            }
        }
    }
}

/// Extract the cover image's content hash from an album's `AbstractData`.
/// Returns `None` for media items or albums without a cover.
/// The cover's `asset_id` resolves to its identity `AssetRecord`, whose
/// `content_hash` is the display rule `object.id` composition would produce.
pub fn cover_content_hash_from_data(abstract_data: &AbstractData) -> Option<ArrayString<64>> {
    let cover_asset_id = match abstract_data {
        AbstractData::Album(album) => album.metadata.cover?,
        _ => return None,
    };
    let record = crate::storage::asset_store::get_asset_by_id(&cover_asset_id).ok()??;
    Some(record.content_hash.unwrap_or(record.asset_id))
}

#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
pub fn abstract_data_to_timestamp_return(
    mut abstract_data: AbstractData,
    timestamp: i64,
    show_download: bool,
    show_metadata: bool,
    asset_id: ArrayString<64>,
    cover_content_hash: Option<ArrayString<64>>,
    caller: String,
    resolved_share_opt: Option<crate::model::album::ResolvedShare>,
) -> DataBaseTimestampReturn {
    let result = DataBaseTimestampReturn::with_asset_id(
        abstract_data.clone(),
        DEFAULT_PRIORITY_LIST,
        timestamp,
        show_download,
        asset_id,
        cover_content_hash,
        caller.clone(),
        resolved_share_opt.clone(),
    );
    clear_abstract_data_metadata(&mut abstract_data, show_metadata);
    DataBaseTimestampReturn {
        abstract_data,
        timestamp: result.timestamp,
        token: result.token,
        asset_id: asset_id.to_string(),
        cover_hash: result.cover_hash,
    }
}

#[cfg(test)]
mod tests {
    use super::clear_abstract_data_metadata;
    use crate::model::abstract_data::AbstractData;
    use crate::model::image::{ImageCombined, ImageMetadata};
    use crate::model::object::{ObjectSchema, ObjectType};
    use crate::model::response::FileEntry;
    use arrayvec::ArrayString;

    fn img_with_path(is_trashed: bool) -> AbstractData {
        let id = ArrayString::from("test").expect("failed to create ArrayString");
        let mut metadata = ImageMetadata::new(0, 0, 0, "jpg".to_string());
        metadata.path = Some(FileEntry {
            file: "/photos/a.jpg".to_string(),
            modified: 1,
            scan_time: 2,
            is_trashed,
        });
        AbstractData::Image(ImageCombined {
            object: ObjectSchema::new(id, ObjectType::Image),
            metadata,
        })
    }

    fn img_without_path() -> AbstractData {
        let id = ArrayString::from("test").expect("failed to create ArrayString");
        AbstractData::Image(ImageCombined {
            object: ObjectSchema::new(id, ObjectType::Image),
            metadata: ImageMetadata::new(0, 0, 0, "jpg".to_string()),
        })
    }

    /// The file entry survives `clear_abstract_data_metadata` for every view,
    /// for every trash flag: with a single path-primary entry there is no
    /// per-view selection left to do (this property was proven against
    /// the old `keep_view_alias` implementation before it was deleted).
    #[test]
    fn clear_metadata_preserves_single_path_for_every_view() {
        for is_trashed in [false, true] {
            let mut data = img_with_path(is_trashed);
            let before = data.path().cloned();
            clear_abstract_data_metadata(&mut data, true);
            assert_eq!(
                data.path(),
                before.as_ref(),
                "single file entry must survive (is_trashed={is_trashed})"
            );
        }
    }

    /// A missing (`None`) path stays `None` under the response trim.
    #[test]
    fn clear_metadata_keeps_missing_path_none() {
        let mut data = img_without_path();
        clear_abstract_data_metadata(&mut data, true);
        assert!(data.path().is_none());
    }

    /// `show_metadata=false` clears the file entry so a metadata-hiding share
    /// cannot leak the filesystem path.
    #[test]
    fn clear_metadata_false_strips_path() {
        let mut data = img_with_path(false);
        clear_abstract_data_metadata(&mut data, false);
        assert!(data.path().is_none());
    }
}
