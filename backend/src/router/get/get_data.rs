// src/router/get/get_data.rs

use crate::model::response::DataBaseTimestampReturn;
use crate::model::response::{Row, ScrollBarData};
use crate::process::resolve_show_download_and_metadata;
use crate::process::transitor::{
    abstract_data_to_database_timestamp_return, hash_to_abstract_data, index_to_hash,
};
use crate::storage::cache::TREE_SNAPSHOT;
use crate::storage::db::{open_data_table, open_tree_snapshot_table};

use crate::error::{AppError, ErrorKind, ResultExt};
use crate::router::auth::GuardTimestamp;
use crate::router::{AppResult, GuardResult};
use anyhow::Result;
use log::info;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use rocket::serde::json::Json;
use std::time::Instant;

#[utoipa::path(
        get,
        path = "/get/get-data",
        responses(
            (status = 200, description = "Data by timestamp range", body = Vec<DataBaseTimestampReturn>),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[get("/get/get-data?<timestamp>&<start>&<end>")]
pub async fn get_data(
    guard_timestamp: GuardResult<GuardTimestamp>,
    timestamp: i64,
    start: usize,
    mut end: usize,
) -> AppResult<Json<Vec<DataBaseTimestampReturn>>> {
    let guard_timestamp = guard_timestamp?;
    tokio::task::spawn_blocking(move || {
        let start_time = Instant::now();

        let resolved_share_opt = guard_timestamp.claims.resolved_share_opt;
        let (show_download, show_metadata) = resolve_show_download_and_metadata(resolved_share_opt);

        let data_table = open_data_table();
        let tree_snapshot = open_tree_snapshot_table(timestamp)
            .or_raise(|| (ErrorKind::Database, "Failed to open tree snapshot table"))?;

        end = end.min(tree_snapshot.len());

        if start >= end {
            return Ok(Json(vec![]));
        }

        let database_timestamp_return_list: Result<Vec<_>, AppError> = (start..end)
            .into_par_iter()
            .map(|index| {
                let hash = index_to_hash(&tree_snapshot, index).or_raise(|| {
                    (
                        ErrorKind::Database,
                        format!("Failed to map index {index} to hash"),
                    )
                })?;

                let abstract_data = hash_to_abstract_data(&data_table, hash).or_raise(|| {
                    (
                        ErrorKind::Database,
                        format!("Failed to retrieve data for hash {hash}"),
                    )
                })?;

                let database_timestamp_return = abstract_data_to_database_timestamp_return(
                    abstract_data,
                    timestamp,
                    show_download,
                    show_metadata,
                );
                Ok(database_timestamp_return)
            })
            .collect();

        let duration = format!("{:?}", start_time.elapsed());
        info!(duration = &*duration; "Get data: {start} ~ {end}");
        Ok(Json(database_timestamp_return_list?))
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))?
}

#[utoipa::path(
        get,
        path = "/get/get-rows",
        responses(
            (status = 200, description = "Row data", body = Row),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[get("/get/get-rows?<index>&<timestamp>")]
pub async fn get_rows(
    auth: GuardResult<GuardTimestamp>,
    index: usize,
    timestamp: i64,
) -> AppResult<Json<Row>> {
    let _ = auth;
    tokio::task::spawn_blocking(move || {
        let start_time = Instant::now();
        let filtered_rows = TREE_SNAPSHOT
            .read_row(index, timestamp)
            .or_raise(|| (ErrorKind::Database, "Failed to read row from snapshot"))?;
        let duration = format!("{:?}", start_time.elapsed());
        info!(duration = &*duration; "Read rows: index = {index}");
        Ok(Json(filtered_rows))
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))?
}

#[utoipa::path(
        get,
        path = "/get/get-scroll-bar",
        responses(
            (status = 200, description = "Scroll bar data", body = Vec<ScrollBarData>),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[get("/get/get-scroll-bar?<timestamp>")]
#[allow(clippy::needless_pass_by_value)]
pub fn get_scroll_bar(
    auth: GuardResult<GuardTimestamp>,
    timestamp: i64,
) -> Json<Vec<ScrollBarData>> {
    let _ = auth;
    let scrollbar_data = TREE_SNAPSHOT.read_scrollbar(timestamp);
    Json(scrollbar_data)
}
