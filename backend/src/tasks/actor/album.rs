use crate::model::abstract_data::AbstractData;
use crate::model::album::AlbumCombined;
use crate::model::asset::AssetRecord;
use crate::model::metadata_record::{MetadataRecord, compose_abstract_data, to_metadata_record};
use crate::model::response::DatabaseTimestamp;
use crate::storage::db::ASSET_BY_ID;
use crate::storage::db::METADATA_TABLE;
use crate::storage::db::TREE;
use anyhow::Context;
use anyhow::Result;
use arrayvec::ArrayString;
use log::debug;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use redb::ReadableTable;

fn read_durable_album(album_id: &ArrayString<64>) -> Result<Option<AlbumCombined>> {
    use redb::ReadableDatabase;
    let read_txn = TREE
        .in_disk
        .begin_read()
        .context("begin_read failed (album)")?;
    let metadata_table = read_txn
        .open_table(METADATA_TABLE)
        .context("open METADATA_TABLE failed (album)")?;
    let id_table = read_txn
        .open_table(ASSET_BY_ID)
        .context("open ASSET_BY_ID failed (album)")?;

    let record = id_table
        .get(&**album_id)?
        .and_then(|guard| serde_json::from_str::<AssetRecord>(guard.value()).ok());
    let payload = metadata_table.get(&**album_id)?.map(|guard| guard.value());

    Ok(match (&record, &payload) {
        (Some(record), Some(MetadataRecord::Album(_))) => {
            match compose_abstract_data(record, payload.as_ref()) {
                AbstractData::Album(album) => Some(album),
                _ => None,
            }
        }
        _ => None,
    })
}

#[quint::operation("OP-8")]
pub fn album_task(album_id: ArrayString<64>, ref_data: &[DatabaseTimestamp]) -> Result<()> {
    debug!("Perform album self-update");
    // Phase 1: read durable album under a short read txn.
    let read_album = read_durable_album(&album_id)?;
    // Phase 2: rayon stats outside any txn.
    let phase1_found_album = read_album.is_some();
    let computed_album: Option<AlbumCombined> = read_album.map(|mut album| {
        album.object.pending = true;
        album.self_update_with(ref_data);
        album.object.pending = false;
        album
    });
    // Deleted-album asset ids (only if phase 1 found no album).
    let deleted_asset_ids: Option<Vec<ArrayString<64>>> = if phase1_found_album {
        None
    } else {
        Some(
            ref_data
                .par_iter()
                .filter_map(|dt| match &dt.abstract_data {
                    AbstractData::Image(img) if img.metadata.album == Some(album_id) => {
                        Some(dt.asset_id)
                    }
                    AbstractData::Video(vid) if vid.metadata.album == Some(album_id) => {
                        Some(dt.asset_id)
                    }
                    _ => None,
                })
                .collect(),
        )
    };
    // Phase 3: write txn reads + insert/clear.
    let txn = TREE
        .in_disk
        .begin_write()
        .context("begin_write failed (album)")?;
    {
        let mut metadata_table = txn.open_table(METADATA_TABLE)?;
        let mut id_table = txn.open_table(ASSET_BY_ID)?;
        let record = id_table
            .get(&*album_id)?
            .and_then(|guard| serde_json::from_str::<AssetRecord>(guard.value()).ok());
        let payload = metadata_table.get(&*album_id)?.map(|guard| guard.value());
        let live_album = match (&record, &payload) {
            (Some(record), Some(MetadataRecord::Album(_))) => {
                match compose_abstract_data(record, payload.as_ref()) {
                    AbstractData::Album(album) => Some(album),
                    _ => None,
                }
            }
            _ => None,
        };
        if live_album.is_some() {
            let Some(computed) = computed_album else {
                // Phase 1/3 disagree: album appeared between txns. Re-mark
                // pending, drop txn uncommitted, no compute in write txn.
                crate::process::dir_album::mark_album_for_update(album_id);
                return Ok(());
            };
            let album_to_write = computed;
            metadata_table.insert(
                &*album_id,
                to_metadata_record(&AbstractData::Album(album_to_write)),
            )?;
        } else if let Some(deleted_asset_ids) = deleted_asset_ids {
            for asset_id in deleted_asset_ids {
                let existing = id_table
                    .get(&*asset_id)?
                    .map(|guard| guard.value().to_string());
                let Some(json) = existing else {
                    continue;
                };
                let Ok(mut record) = serde_json::from_str::<AssetRecord>(&json) else {
                    continue;
                };
                if record.album_id == Some(album_id) {
                    record.album_id = None;
                    let updated = serde_json::to_string(&record)?;
                    id_table.insert(&*asset_id, updated.as_str())?;
                }
            }
        }
    }
    txn.commit().context("commit failed (album)")?;
    crate::storage::tree_view::bump_tree_epoch();
    Ok(())
}
