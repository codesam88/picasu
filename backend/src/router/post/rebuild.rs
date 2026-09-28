// src/router/post/rebuild.rs

use rocket::post;
use rocket::serde::json::Json;

use crate::error::{AppError, ErrorKind, ResultExt};
use crate::openapi_components::Unauthorized;
use crate::process::rebuild::{RebuildStats, rebuild_from_filesystem};
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::{AppResult, GuardResult};
use crate::storage::files::get_resolved_image_home;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::update_tree::UpdateTreeTask;

/// Rebuild the asset tables and the metadata cache from the filesystem under
/// `IMAGE_HOME`.
///
/// `rebuild_from_filesystem` clears `ASSET_BY_PATH`, `ASSET_BY_ID`,
/// `DUPE_INDEX` and `METADATA_TABLE`, walks the image root, and repopulates all
/// four: identity from the walk, metadata from the same pipeline the incremental
/// indexer runs. This route then waits for an in-memory tree refresh so the
/// response does not race subsequent `prefetch`/`get-data` calls.
///
/// The response carries the per-file outcome. `metadataIndexed` against
/// `mediaCreated` is how a caller tells a rebuild that produced usable metadata
/// from one that only reissued identity, and `metadataFailures` names the files
/// that could not be processed.
#[utoipa::path(
        tag = "index",
        responses(
            (status = 200, description = "Rebuild complete", body = RebuildStats),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
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

    // Filesystem walk plus one ExifTool read and one thumbnail write per media
    // file: blocking work, off the async runtime.
    let stats = tokio::task::spawn_blocking(move || rebuild_from_filesystem(&image_root))
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to join rebuild task"))??;

    BATCH_COORDINATOR
        .execute_batch_waiting(UpdateTreeTask)
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to execute update tree task"))?;

    Ok(Json(stats))
}
