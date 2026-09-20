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

pub fn index_to_hash(tree_snapshot: &MyCow, index: usize) -> Result<ArrayString<64>> {
    if index >= tree_snapshot.len() {
        return Err(anyhow::anyhow!("Index out of bounds: {index}"));
    }
    let hash = tree_snapshot.get_hash(index)?;
    Ok(hash)
}

#[allow(dead_code)]
pub fn index_to_asset_id(tree_snapshot: &MyCow, index: usize) -> Result<ArrayString<64>> {
    if index >= tree_snapshot.len() {
        return Err(anyhow::anyhow!("Index out of bounds: {index}"));
    }
    let asset_id = tree_snapshot.get_asset_id(index)?;
    Ok(asset_id)
}

pub fn hash_to_abstract_data(
    data_table: &ReadOnlyTable<&'static str, AbstractData>,
    hash: ArrayString<64>,
) -> Result<AbstractData> {
    if let Some(data) = data_table.get(&*hash)? {
        Ok(data.value())
    } else {
        Err(anyhow::anyhow!("No data found for hash: {hash}"))
    }
}

/// Resolve an `asset_id` to an `AbstractData` record.
///
/// First tries the new `ASSET_BY_ID` store. If the asset exists there,
/// converts it to an `AbstractData` for backward-compatible API responses.
/// Falls back to the old `DATA_TABLE` by hash if no asset is found.
#[allow(dead_code)]
pub fn asset_id_to_abstract_data(
    asset_id: ArrayString<64>,
    data_table: &ReadOnlyTable<&'static str, AbstractData>,
) -> Result<AbstractData> {
    // Try new asset store first.
    if let Some(record) = crate::storage::asset_store::get_asset_by_id(&asset_id)? {
        return Ok(asset_record_to_abstract_data(&record));
    }

    // Fall back to old DATA_TABLE (asset_id might actually be a hash).
    hash_to_abstract_data(data_table, asset_id)
}

/// Convert an `AssetRecord` to an `AbstractData` for API backward compatibility.
/// This is a lossy conversion — EXIF, tags, and other metadata are not preserved.
#[allow(dead_code)]
fn asset_record_to_abstract_data(record: &crate::model::asset::AssetRecord) -> AbstractData {
    use crate::model::album::{AlbumCombined, AlbumMetadata};
    use crate::model::asset::AssetKind;
    use crate::model::image::{ImageCombined, ImageMetadata};
    use crate::model::object::{ObjectSchema, ObjectType};
    use crate::model::response::FileModify;
    use crate::model::video::{VideoCombined, VideoMetadata};

    // Use content hash as ObjectSchema.id for backward compat with thumbnail serving.
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
            let object = ObjectSchema::new(display_id, ObjectType::Album);
            let metadata = AlbumMetadata {
                id: display_id,
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

pub fn abstract_data_to_database_timestamp_return(
    mut abstract_data: AbstractData,
    timestamp: i64,
    show_download: bool,
    show_metadata: bool,
    trashed_view: bool,
) -> DataBaseTimestampReturn {
    let result = DataBaseTimestampReturn::new(
        abstract_data.clone(),
        DEFAULT_PRIORITY_LIST,
        timestamp,
        show_download,
    );
    clear_abstract_data_metadata(&mut abstract_data, show_metadata, trashed_view);
    DataBaseTimestampReturn {
        abstract_data,
        timestamp: result.timestamp,
        token: result.token,
    }
}

pub fn index_to_abstract_data(
    tree_snapshot: &MyCow,
    data_table: &ReadOnlyTable<&'static str, AbstractData>,
    index: usize,
) -> Result<AbstractData> {
    let hash = index_to_hash(tree_snapshot, index)?;
    hash_to_abstract_data(data_table, hash)
}
