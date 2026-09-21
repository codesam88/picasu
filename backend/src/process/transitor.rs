use crate::constant::DEFAULT_PRIORITY_LIST;
use crate::model::abstract_data::AbstractData;
use crate::model::response::{DataBaseTimestampReturn, FileModify};
use crate::storage::cache::MyCow;
use anyhow::Result;
use arrayvec::ArrayString;
use redb::ReadOnlyTable;

/// Trim the alias list to a single surfaced alias, preferring the newest
/// alias matching the requested view (live in the gallery, trashed in the
/// trash view) and falling back to the newest alias overall so the record is
/// never dropped from either view.
fn keep_view_alias(alias: &mut Vec<FileModify>, trashed_view: bool) {
    let chosen = alias
        .iter()
        .filter(|a| a.is_trashed == trashed_view)
        .max_by_key(|a| a.scan_time)
        .or_else(|| alias.iter().max_by_key(|a| a.scan_time))
        .cloned();
    match chosen {
        Some(last_alias) => *alias = vec![last_alias],
        None => alias.clear(),
    }
}

pub fn index_to_asset_id(tree_snapshot: &MyCow, index: usize) -> Result<ArrayString<64>> {
    if index >= tree_snapshot.len() {
        return Err(anyhow::anyhow!("Index out of bounds: {index}"));
    }
    let asset_id = tree_snapshot.get_asset_id(index)?;
    Ok(asset_id)
}

/// Resolve an `asset_id` to an `AbstractData` record via `DATA_TABLE`.
pub fn asset_id_to_abstract_data(
    asset_id: ArrayString<64>,
    data_table: &ReadOnlyTable<&'static str, AbstractData>,
) -> Result<AbstractData> {
    if let Some(data) = data_table.get(&*asset_id)? {
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
            metadata.alias = vec![FileModify {
                file: record.canonical_path.clone(),
                modified: record.modified,
                scan_time: record.scan_time,
                is_trashed: record.is_trashed,
            }];
            AbstractData::Image(ImageCombined { object, metadata })
        }
        AssetKind::Video => {
            let object = ObjectSchema::new(display_id, ObjectType::Video);
            let mut metadata =
                VideoMetadata::new(display_id, record.file_size, 0, 0, record.ext.clone());
            metadata.alias = vec![FileModify {
                file: record.canonical_path.clone(),
                modified: record.modified,
                scan_time: record.scan_time,
                is_trashed: record.is_trashed,
            }];
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

pub fn clear_abstract_data_metadata(
    abstract_data: &mut AbstractData,
    show_metadata: bool,
    trashed_view: bool,
) {
    match abstract_data {
        AbstractData::Image(img) => {
            keep_view_alias(&mut img.metadata.alias, trashed_view);
            if !show_metadata {
                img.metadata.album = None;
                img.object.tags.clear();
                img.metadata.alias.clear();
                img.metadata.exif_vec.clear();
            }
        }
        AbstractData::Video(vid) => {
            keep_view_alias(&mut vid.metadata.alias, trashed_view);
            if !show_metadata {
                vid.metadata.album = None;
                vid.object.tags.clear();
                vid.metadata.alias.clear();
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
/// Looks up the cover image by its `cover` `asset_id` in the data table
/// and returns the image's `object.id` (content hash).
pub fn cover_content_hash_from_data(
    abstract_data: &AbstractData,
    data_table: &ReadOnlyTable<&'static str, AbstractData>,
) -> Option<ArrayString<64>> {
    let cover_asset_id = match abstract_data {
        AbstractData::Album(album) => album.metadata.cover?,
        _ => return None,
    };
    let cover_data = asset_id_to_abstract_data(cover_asset_id, data_table).ok()?;
    Some(cover_data.hash())
}

pub fn abstract_data_to_timestamp_return(
    mut abstract_data: AbstractData,
    timestamp: i64,
    show_download: bool,
    show_metadata: bool,
    trashed_view: bool,
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
    clear_abstract_data_metadata(&mut abstract_data, show_metadata, trashed_view);
    DataBaseTimestampReturn {
        abstract_data,
        timestamp: result.timestamp,
        token: result.token,
        asset_id: asset_id.to_string(),
        cover_hash: result.cover_hash,
    }
}
