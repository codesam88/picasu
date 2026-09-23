use crate::error::handle_error;
use crate::model::abstract_data::AbstractData;
use crate::model::asset::AssetRecord;
use crate::model::metadata_record::{MetadataRecord, compose_abstract_data, to_metadata_record};
use crate::storage::db::ASSET_BY_ID;
use crate::storage::db::METADATA_TABLE;
use crate::storage::db::TREE;
use anyhow::Context;
use anyhow::Result;
use arrayvec::ArrayString;
use log::info;
use mini_executor::Task;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use redb::ReadableTable;
use tokio::task::spawn_blocking;

pub struct AlbumSelfUpdateTask {
    album_id: ArrayString<64>,
}

impl AlbumSelfUpdateTask {
    pub fn new(album_id: ArrayString<64>) -> Self {
        Self { album_id }
    }
}

impl Task for AlbumSelfUpdateTask {
    type Output = Result<()>;

    async fn run(self) -> Self::Output {
        spawn_blocking(move || album_task(self.album_id))
            .await
            .expect("blocking task panicked")
            .map_err(|err| handle_error(err.context("Failed to run album task")))
    }
}

pub fn album_task(album_id: ArrayString<64>) -> Result<()> {
    info!("Perform album self-update");

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

        let album_combined = match (&record, &payload) {
            (Some(record), Some(MetadataRecord::Album(_))) => {
                match compose_abstract_data(record, payload.as_ref()) {
                    AbstractData::Album(album) => Some(album),
                    _ => None,
                }
            }
            _ => None,
        };

        if let Some(mut album) = album_combined {
            album.object.pending = true;
            album.self_update();
            album.object.pending = false;
            metadata_table.insert(&*album_id, to_metadata_record(&AbstractData::Album(album)))?;
        } else {
            // Album has been deleted
            let ref_data = TREE.in_memory.read().expect("lock poisoned");

            // Collect all data contained in this album
            let asset_ids: Vec<_> = ref_data
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
                .collect();

            // Clear album membership on the surviving assets' identity
            // records (membership is owned by `AssetRecord.album_id`).
            for asset_id in asset_ids {
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
    Ok(())
}
