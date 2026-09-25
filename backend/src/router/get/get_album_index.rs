use rocket::get;
use rocket::serde::json::Json;

use crate::openapi_components::Unauthorized;
use crate::router::auth::GuardAuth;
use crate::tasks::actor::album_index::{AlbumIndexStatus, album_index_status};

#[utoipa::path(
        get,
        path = "/get/index/status",
        responses(
            (status = 200, description = "Album index status", body = AlbumIndexStatus),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
        )
    )
]
#[get("/get/index/status")]
pub fn get_album_index_status(_auth: GuardAuth) -> Json<AlbumIndexStatus> {
    Json(album_index_status())
}
