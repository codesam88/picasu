use rocket::get;
use rocket::serde::json::Json;

use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardAuth;
use crate::tasks::actor::album_index::{AlbumIndexStatus, album_index_status};

/// Report the state of the most recently started album-indexing job.
///
/// The body carries the job's state (`idle`, `running`, `completed`,
/// `canceled` or `failed`), the root it walks, the `scanned` / `matched` /
/// `processed` / `failed` counters, `startedAt` / `finishedAt` epoch
/// milliseconds and `cancelRequested`, so an album-index run can be polled
/// until it settles.
///
/// Corner cases: before the first run the state is `idle` with zeroed counters
/// and no timestamps. Only the album-index job writes this status: a new run
/// overwrites it, and single-image indexing never appears here.
///
/// Errors: 401 no valid admin credentials.
#[utoipa::path(
        tag = "index",
        responses(
            (status = 200, description = "Album index status", body = AlbumIndexStatus),
            (status = 401, response = Unauthorized),
        )
    )
]
#[get("/get/index/status")]
pub fn get_album_index_status(_auth: GuardAuth) -> Json<AlbumIndexStatus> {
    Json(album_index_status())
}
