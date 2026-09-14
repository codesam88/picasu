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
