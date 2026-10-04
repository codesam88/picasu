use rocket::http::Status;
use rocket::post;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::{AppResult, GuardResult};
use crate::tasks::actor::album_index::{cancel_album_index, index_album};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct IndexAlbumRequest {
    album: String,
}

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
pub struct IndexImageRequest {
    image: String,
    album: Option<String>,
}

/// Index every media file under a directory tree in the background.
///
/// `album` is a directory path relative to `IMAGE_HOME`, where `"/"` selects
/// the root. The walk is asynchronous: the response returns once the job is
/// accepted, and progress — the scanned, matched, processed and failed
/// counters plus the final state — is reported by `GET /get/index/status`.
///
/// Corner cases: One album-index job runs at a time, so a request made while
/// another job is running is a 409. The walk does not stop at a file it cannot
/// read or decode; it counts the failure and carries on.
///
/// Errors: 400 no `imagePath` configured, a path that is missing or not a
/// directory, or a Picasu internal data directory — 401 missing or invalid
/// credentials — 405 read-only mode — 409 an index job is already running.
#[utoipa::path(
        tag = "index",
        request_body = IndexAlbumRequest,
        responses(
            (status = 202, description = "Album indexing started"),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 405, description = "Read-only mode"),
            (status = 409, description = "An index job is already running"),
        )
    )
]
#[post("/post/index/album", data = "<req>")]
pub fn index_album_handler(
    _auth: GuardAuth,
    read_only: GuardResult<GuardReadOnlyMode>,
    req: Json<IndexAlbumRequest>,
) -> AppResult<Status> {
    let _ = read_only?;
    index_album(&req.into_inner().album)?;
    Ok(Status::Accepted)
}

/// Index a single image by its path relative to `IMAGE_HOME`.
///
/// The request returns as soon as the indexing task is spawned, so it reports
/// only that the work was started. `image` is the path below `IMAGE_HOME`,
/// and `album` optionally overrides the album the image is filed under.
///
/// Corner cases: The task runs detached from the request, so an indexing
/// failure is logged rather than returned and leaves no entry in
/// `GET /get/index/status`, which tracks album-index jobs only.
///
/// Errors: 400 unusable request body — 401 missing or invalid credentials —
/// 405 read-only mode.
#[utoipa::path(
        tag = "index",
        request_body = IndexImageRequest,
        responses(
            (status = 202, description = "Image indexing started"),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 405, description = "Read-only mode"),
        )
    )
]
#[post("/post/index/image", data = "<req>")]
pub fn index_image_handler(
    _auth: GuardAuth,
    read_only: GuardResult<GuardReadOnlyMode>,
    req: Json<IndexImageRequest>,
) -> AppResult<Status> {
    let _ = read_only?;
    let inner = req.into_inner();
    let src = PathBuf::from(inner.image);
    let dst = inner.album.map(PathBuf::from);
    rocket::tokio::spawn(async move {
        if let Err(e) = crate::workflow::index_image(&src, dst.as_deref()).await {
            log::error!("index_image failed: {e}");
        }
    });
    Ok(Status::Accepted)
}

/// Request cancellation of the running album index job.
///
/// Stores a cancel flag that the running walk checks as it goes; the response
/// returns as soon as the flag is set, while the job itself keeps walking
/// until it reaches its next directory entry.
///
/// Corner cases: Cancellation is cooperative, not immediate. While the walk
/// winds down the job reports `cancelRequested` with its state still `running`;
/// the state settles as `canceled` only once the walk has actually stopped.
///
/// Errors: 400 malformed request — 401 missing or invalid credentials —
/// 404 no index job is active.
#[utoipa::path(
        tag = "index",
        responses(
            (status = 200, description = "Album index cancelled"),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 404, description = "No active index job"),
        )
    )
]
#[post("/post/index/cancel")]
pub fn cancel_album_index_handler(_auth: GuardAuth) -> AppResult<Status> {
    cancel_album_index()?;
    Ok(Status::Ok)
}
