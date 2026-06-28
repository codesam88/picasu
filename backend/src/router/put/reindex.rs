use arrayvec::ArrayString;
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use rocket::http::Status;

use crate::constant::PROCESS_BATCH_NUMBER;
use crate::error::{ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::process::index::regenerate_metadata_for_image;
use crate::process::index::regenerate_metadata_for_video;
use crate::router::AppResult;
use crate::router::GuardResult;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::storage::cache::TREE_SNAPSHOT;
use crate::storage::db::open_data_table;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::INDEX_COORDINATOR;
use crate::tasks::actor::album::AlbumSelfUpdateTask;
use crate::tasks::batcher::flush_tree::FlushTreeTask;
use crate::tasks::batcher::update_tree::UpdateTreeTask;

use log::{error, info};
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct RegenerateData {
    index_array: Vec<usize>,
    timestamp: i64,
}

#[utoipa::path(
        post,
        path = "/put/reindex",
        request_body = RegenerateData,
        responses(
            (status = 200, description = "Reindex complete"),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[post("/put/reindex", format = "json", data = "<json_data>")]
pub async fn reindex(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    json_data: Json<RegenerateData>,
) -> AppResult<Status> {
    let _ = auth?;
    let _ = read_only_mode?;
    let json_data = json_data.into_inner();
    tokio::task::spawn_blocking(move || {
        let data_table = open_data_table();
        let reduced_data_vec = TREE_SNAPSHOT
            .read_tree_snapshot(json_data.timestamp)
            .expect("failed to read tree snapshot");
        let hash_vec: Vec<ArrayString<64>> = json_data
            .index_array
            .par_iter()
            .map(|index| {
                reduced_data_vec
                    .get_hash(*index)
                    .expect("failed to get hash from reduced data")
            })
            .collect();
        let total_batches = hash_vec.len().div_ceil(PROCESS_BATCH_NUMBER);

        for (i, batch) in hash_vec.chunks(PROCESS_BATCH_NUMBER).enumerate() {
            info!("Processing batch {}/{}", i + 1, total_batches);

            let data_list: Vec<_> = batch
                .into_par_iter()
                .filter_map(|&hash| {
                    if let Some(guard) = data_table.get(&*hash).expect("failed to get record") {
                        let mut abstract_data = guard.value();
                        match &abstract_data {
                            AbstractData::Image(_) => {
                                match regenerate_metadata_for_image(&mut abstract_data) {
                                    Ok(()) => Some(abstract_data),
                                    Err(_) => None,
                                }
                            }
                            AbstractData::Video(_) => {
                                match regenerate_metadata_for_video(&mut abstract_data) {
                                    Ok(()) => Some(abstract_data),
                                    Err(_) => None,
                                }
                            }
                            AbstractData::Album(_) => {
                                // album_self_update already will commit
                                INDEX_COORDINATOR.execute_detached(AlbumSelfUpdateTask::new(hash));
                                None
                            }
                        }
                    } else {
                        error!("Reindex failed: cannot find data with hash/id: {hash}");
                        None
                    }
                })
                .collect();
            BATCH_COORDINATOR.execute_batch_detached(FlushTreeTask::insert(data_list));
        }
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))?;

    BATCH_COORDINATOR.execute_batch_detached(UpdateTreeTask);
    Ok(Status::Ok)
}
