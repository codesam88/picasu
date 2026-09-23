// src/router/post/rebuild.rs

use rocket::post;
use rocket::serde::json::Json;

use crate::error::{AppError, ErrorKind, ResultExt};
use crate::process::rebuild::{RebuildStats, rebuild_from_filesystem};
use crate::process::transitor::asset_record_to_abstract_data;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::{AppResult, GuardResult};
use crate::storage::asset_store;
use crate::storage::db::{METADATA_TABLE, TREE};
use crate::storage::files::get_resolved_image_home;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::update_tree::UpdateTreeTask;

/// Rebuild the asset tables from the filesystem under `IMAGE_HOME`.
///
/// Clears `ASSET_BY_PATH`/`ASSET_BY_ID`/`DUPE_INDEX`, walks the image root,
/// and repopulates them. Then rewrites `METADATA_TABLE` from the fresh
/// `AssetRecord`s (rebuild assigns new `asset_id`s, so stale rows keyed by
/// the old ids must not remain) and waits for an in-memory tree refresh so
/// the response does not race subsequent `prefetch`/`get-data` calls.
#[utoipa::path(
        post,
        path = "/post/rebuild",
        responses(
            (status = 200, description = "Rebuild complete", body = RebuildStats),
            (status = 400, description = "Invalid input"),
            (status = 405, description = "Read-only mode"),
        )
    )
]
#[post("/post/rebuild")]
pub async fn rebuild_handler(
    _auth: GuardAuth,
    read_only: GuardResult<GuardReadOnlyMode>,
) -> AppResult<Json<RebuildStats>> {
    let _ = read_only?;

    let image_root = get_resolved_image_home()
        .ok_or_else(|| AppError::new(ErrorKind::Internal, "IMAGE_HOME is not configured"))?;

    let stats = tokio::task::spawn_blocking(move || {
        let stats = rebuild_from_filesystem(&image_root)?;
        sync_metadata_table()?;
        anyhow::Ok(stats)
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join rebuild task"))??;

    BATCH_COORDINATOR
        .execute_batch_waiting(UpdateTreeTask)
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to execute update tree task"))?;

    Ok(Json(stats))
}

/// Replace every `METADATA_TABLE` row with one derived from the current
/// `ASSET_BY_ID` records. Only the metadata-only payload is written;
/// identity stays on the records (media rows carry no album field — album
/// membership is `AssetRecord.album_id`).
fn sync_metadata_table() -> anyhow::Result<()> {
    use crate::model::metadata_record::to_metadata_record;
    use redb::ReadableTable;

    let records = asset_store::get_all_assets()?;

    let txn = TREE.in_disk.begin_write()?;
    {
        let existing: Vec<String> = {
            let table = txn.open_table(METADATA_TABLE)?;
            table
                .iter()?
                .filter_map(|row| row.ok().map(|(k, _)| k.value().to_string()))
                .collect()
        };

        let mut table = txn.open_table(METADATA_TABLE)?;
        for key in &existing {
            table.remove(key.as_str())?;
        }
        for record in &records {
            let data = asset_record_to_abstract_data(record);
            table.insert(record.asset_id.as_str(), to_metadata_record(&data))?;
        }
    }
    txn.commit()?;
    Ok(())
}
