use rocket::http::Status;
use rocket::post;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

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

/// Walk a directory under `IMAGE_HOME` and index all media files in the
/// background.  `album` is a path relative to `IMAGE_HOME` — use `"/"` for
/// the root.  Status can be polled via `GET /get/index/status`.
#[utoipa::path(
        post,
        path = "/post/index/album",
        request_body = IndexAlbumRequest,
        responses(
            (status = 200, description = "Album indexing started"),
            (status = 400, description = "Invalid input"),
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

/// Index a single image by its path relative to `IMAGE_HOME`.  Runs in the
/// background; returns `202 Accepted` immediately.
#[utoipa::path(
        post,
        path = "/post/index/image",
        request_body = IndexImageRequest,
        responses(
            (status = 200, description = "Image indexing started"),
            (status = 400, description = "Invalid input"),
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

/// Cancel a running album index job.
#[utoipa::path(
        post,
        path = "/post/index/cancel",
        responses(
            (status = 200, description = "Album index cancelled"),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[post("/post/index/cancel")]
pub fn cancel_album_index_handler(_auth: GuardAuth) -> AppResult<Status> {
    cancel_album_index()?;
    Ok(Status::Ok)
}
