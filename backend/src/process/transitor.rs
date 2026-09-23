use crate::constant::DEFAULT_PRIORITY_LIST;
use crate::model::abstract_data::AbstractData;
use crate::model::response::DataBaseTimestampReturn;
use crate::storage::cache::MyCow;
use anyhow::Result;
use arrayvec::ArrayString;
use redb::ReadOnlyTable;

pub fn index_to_asset_id(tree_snapshot: &MyCow, index: usize) -> Result<ArrayString<64>> {
    if index >= tree_snapshot.len() {
        return Err(anyhow::anyhow!("Index out of bounds: {index}"));
    }
    let asset_id = tree_snapshot.get_asset_id(index)?;
    Ok(asset_id)
}

/// Resolve an `asset_id` to an `AbstractData` record via `METADATA_TABLE`.
pub fn asset_id_to_abstract_data(
    asset_id: ArrayString<64>,
    metadata_table: &ReadOnlyTable<&'static str, AbstractData>,
) -> Result<AbstractData> {
    if let Some(data) = metadata_table.get(&*asset_id)? {
        return Ok(data.value());
    }

    Err(anyhow::anyhow!("No data found for asset_id: {asset_id}"))
}

/// Convert an `AssetRecord` to an `AbstractData` for API responses.
/// This is a lossy conversion — EXIF, tags, and other metadata are not preserved.
/// Uses content hash as `object.id` for media items (required for compressed
/// thumbnail path resolution). Uses `asset_id` for albums (no thumbnails).
pub fn asset_record_to_abstract_data(record: &crate::model::asset::AssetRecord) -> AbstractData {
    use crate::model::album::{AlbumCombined, AlbumMetadata};
    use crate::model::asset::AssetKind;
    use crate::model::image::{ImageCombined, ImageMetadata};
    use crate::model::object::{ObjectSchema, ObjectType};
    use crate::model::response::FileModify;
    use crate::model::video::{VideoCombined, VideoMetadata};

    // Media items use content hash as object.id (required for compressed
    // thumbnail path resolution). Albums use asset_id (no thumbnails).
    let display_id = record.content_hash.unwrap_or(record.asset_id);

    match record.kind {
        AssetKind::Image => {
            let object = ObjectSchema::new(display_id, ObjectType::Image);
            let mut metadata =
                ImageMetadata::new(display_id, record.file_size, 0, 0, record.ext.clone());
            metadata.alias = Some(FileModify {
                file: record.canonical_path.clone(),
                modified: record.modified,
                scan_time: record.scan_time,
                is_trashed: record.is_trashed,
            });
            AbstractData::Image(ImageCombined { object, metadata })
        }
        AssetKind::Video => {
            let object = ObjectSchema::new(display_id, ObjectType::Video);
            let mut metadata =
                VideoMetadata::new(display_id, record.file_size, 0, 0, record.ext.clone());
            metadata.alias = Some(FileModify {
                file: record.canonical_path.clone(),
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
                item_count: 0,
                item_size: 0,
                share_list: std::collections::HashMap::new(),
                dir_path: record.canonical_path.clone(),
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
/// The row carries identity, dimensions, alias, album membership, and the
/// cache-bust/processing keys (`update_at`, `pending`) — tags, EXIF, and
/// description are deliberately absent and must be fetched via
/// `GET /get/metadata/{assetId}` (detail/sidebar). Rating, favorite, and
/// archived flags likewise live behind the detail endpoint.
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
                img.metadata.alias = None;
                img.metadata.exif_vec.clear();
            }
        }
        AbstractData::Video(vid) => {
            if !show_metadata {
                vid.metadata.album = None;
                vid.object.tags.clear();
                vid.metadata.alias = None;
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
/// Looks up the cover image by its `cover` `asset_id` in `METADATA_TABLE`
/// and returns the image's `object.id` (content hash).
pub fn cover_content_hash_from_data(
    abstract_data: &AbstractData,
    metadata_table: &ReadOnlyTable<&'static str, AbstractData>,
) -> Option<ArrayString<64>> {
    let cover_asset_id = match abstract_data {
        AbstractData::Album(album) => album.metadata.cover?,
        _ => return None,
    };
    let cover_data = asset_id_to_abstract_data(cover_asset_id, metadata_table).ok()?;
    Some(cover_data.hash())
}

#[cfg(test)]
mod tests {
    use super::clear_abstract_data_metadata;
    use crate::model::abstract_data::AbstractData;
    use crate::model::image::{ImageCombined, ImageMetadata};
    use crate::model::object::{ObjectSchema, ObjectType};
    use crate::model::response::FileModify;
    use arrayvec::ArrayString;

    fn img_with_alias(is_trashed: bool) -> AbstractData {
        let id = ArrayString::from("test").expect("failed to create ArrayString");
        let mut metadata = ImageMetadata::new(id, 0, 0, 0, "jpg".to_string());
        metadata.alias = Some(FileModify {
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

    fn img_without_alias() -> AbstractData {
        let id = ArrayString::from("test").expect("failed to create ArrayString");
        AbstractData::Image(ImageCombined {
            object: ObjectSchema::new(id, ObjectType::Image),
            metadata: ImageMetadata::new(id, 0, 0, 0, "jpg".to_string()),
        })
    }

    /// The alias survives `clear_abstract_data_metadata` for every view, for
    /// every trash flag: with a single path-primary alias there is no
    /// per-view alias selection left to do (this property was proven against
    /// the old `keep_view_alias` implementation before it was deleted).
    #[test]
    fn clear_metadata_preserves_single_alias_for_every_view() {
        for is_trashed in [false, true] {
            let mut data = img_with_alias(is_trashed);
            let before = data.alias().cloned();
            clear_abstract_data_metadata(&mut data, true);
            assert_eq!(
                data.alias(),
                before.as_ref(),
                "single alias must survive (is_trashed={is_trashed})"
            );
        }
    }

    /// A pruned (`None`) alias stays `None` under the response trim.
    #[test]
    fn clear_metadata_keeps_pruned_alias_none() {
        let mut data = img_without_alias();
        clear_abstract_data_metadata(&mut data, true);
        assert!(data.alias().is_none());
    }

    /// `show_metadata=false` clears the alias so a metadata-hiding share
    /// cannot leak the filesystem path.
    #[test]
    fn clear_metadata_false_strips_alias() {
        let mut data = img_with_alias(false);
        clear_abstract_data_metadata(&mut data, false);
        assert!(data.alias().is_none());
    }
}

pub fn abstract_data_to_timestamp_return(
    mut abstract_data: AbstractData,
    timestamp: i64,
    show_download: bool,
    show_metadata: bool,
    asset_id: ArrayString<64>,
    cover_content_hash: Option<ArrayString<64>>,
) -> DataBaseTimestampReturn {
    let result = DataBaseTimestampReturn::with_asset_id(
        abstract_data.clone(),
        DEFAULT_PRIORITY_LIST,
        timestamp,
        show_download,
        asset_id,
        cover_content_hash,
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
