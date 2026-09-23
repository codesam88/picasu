// src/router/get/get_data.rs

use crate::model::asset::{AssetKind, AssetRecord};
use crate::model::response::DataBaseTimestampReturn;
use crate::model::response::{Row, ScrollBarData};
use crate::process::resolve_show_download_and_metadata;
use crate::process::transitor::{
    asset_id_to_abstract_data, cover_content_hash_from_data, lean_media_abstract_data,
};
use crate::storage::cache::TREE_SNAPSHOT;
use crate::storage::db::{ASSET_BY_ID, TREE, open_metadata_table, open_tree_snapshot_table};

use crate::error::{AppError, ErrorKind, ResultExt};
use crate::router::auth::GuardTimestamp;
use crate::router::{AppResult, GuardResult};
use anyhow::Result;
use log::info;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use redb::ReadableDatabase;
use rocket::serde::json::Json;
use std::time::Instant;

/// Serve one page of timeline/list rows for a snapshot timestamp.
///
/// Phase 14 lean read path: media rows are built from the snapshot's
/// `ReducedData` plus the lean `ASSET_BY_ID` record — no per-row
/// `METADATA_TABLE` (full `AbstractData`) read, and no tags/EXIF/description
/// on the payload (those are served by `GET /get/metadata/{assetId}`).
/// Album rows still read `METADATA_TABLE` because tiles need their stored
/// title/cover/counts.
#[utoipa::path(
        get,
        path = "/get/get-data",
        responses(
            (status = 200, description = "Data by timestamp range", body = Vec<DataBaseTimestampReturn>),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[get("/get/get-data?<timestamp>&<start>&<end>&<trashed>")]
pub async fn get_data(
    guard_timestamp: GuardResult<GuardTimestamp>,
    timestamp: i64,
    start: usize,
    mut end: usize,
    trashed: Option<bool>,
) -> AppResult<Json<Vec<DataBaseTimestampReturn>>> {
    let guard_timestamp = guard_timestamp?;
    tokio::task::spawn_blocking(move || {
        let start_time = Instant::now();

        let trashed_view = trashed.unwrap_or(false);
        let resolved_share_opt = guard_timestamp.claims.resolved_share_opt;
        let (show_download, show_metadata) = resolve_show_download_and_metadata(resolved_share_opt);

        let metadata_table = open_metadata_table();
        let tree_snapshot = open_tree_snapshot_table(timestamp)
            .or_raise(|| (ErrorKind::Database, "Failed to open tree snapshot table"))?;

        // Lean identity records for media rows (path, kind, times, album).
        let asset_txn = TREE
            .in_disk
            .begin_read()
            .or_raise(|| (ErrorKind::Database, "Failed to begin read transaction"))?;
        let asset_by_id = asset_txn
            .open_table(ASSET_BY_ID)
            .or_raise(|| (ErrorKind::Database, "Failed to open ASSET_BY_ID"))?;

        end = end.min(tree_snapshot.len());

        if start >= end {
            return Ok(Json(vec![]));
        }

        let database_timestamp_return_list: Result<Vec<_>, AppError> = (start..end)
            .into_par_iter()
            .map(|index| {
                let reduced = tree_snapshot.get_reduced(index).or_raise(|| {
                    (
                        ErrorKind::Database,
                        format!("Failed to read snapshot entry for index {index}"),
                    )
                })?;
                let asset_id = reduced.asset_id;

                let record_json = asset_by_id
                    .get(&*asset_id)
                    .or_raise(|| {
                        (
                            ErrorKind::Database,
                            format!("Failed to read ASSET_BY_ID for asset_id {asset_id}"),
                        )
                    })?
                    .ok_or_else(|| {
                        AppError::new(
                            ErrorKind::Database,
                            format!("No ASSET_BY_ID record for asset_id {asset_id}"),
                        )
                    })?;
                let record: AssetRecord =
                    serde_json::from_str(record_json.value()).or_raise(|| {
                        (
                            ErrorKind::Database,
                            format!("Failed to parse AssetRecord for asset_id {asset_id}"),
                        )
                    })?;

                let (abstract_data, cover_content_hash) = match record.kind {
                    // Albums keep their full metadata row: tiles need
                    // title/cover/counts, and cover_hash resolves via
                    // METADATA_TABLE.
                    AssetKind::Album => {
                        let abstract_data = asset_id_to_abstract_data(asset_id, &metadata_table)
                            .or_raise(|| {
                                (
                                    ErrorKind::Database,
                                    format!("Failed to retrieve album for asset_id {asset_id}"),
                                )
                            })?;
                        let cover = cover_content_hash_from_data(&abstract_data, &metadata_table);
                        (abstract_data, cover)
                    }
                    // Media rows: lean construction, no metadata read.
                    AssetKind::Image | AssetKind::Video => {
                        (lean_media_abstract_data(&record, &reduced), None)
                    }
                };

                let mut database_timestamp_return =
                    crate::process::transitor::abstract_data_to_timestamp_return(
                        abstract_data,
                        timestamp,
                        show_download,
                        show_metadata,
                        trashed_view,
                        asset_id,
                        cover_content_hash,
                    );
                // Row timestamps mirror the tree snapshot's date (computed
                // from the full in-memory record when the snapshot was taken),
                // so lean rows keep the EXIF-derived sort date.
                database_timestamp_return.timestamp = reduced.date;
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
