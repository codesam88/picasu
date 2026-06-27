use crate::error::{AppError, ErrorKind, ResultExt};
use crate::process::dir_album::{
    get_dir_path_for_album, get_or_create_dir_album, mark_album_for_update,
};
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::router::{AppResult, GuardResult};
use crate::tasks::INDEX_COORDINATOR;
use crate::tasks::actor::album::AlbumSelfUpdateTask;
use arrayvec::ArrayString;
use rocket::serde::{Deserialize, Serialize, json::Json};
use std::fs;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct CreateDirAlbumData {
    #[schema(value_type = String)]
    pub parent_album_id: ArrayString<64>,
    pub name: String,
}

/// Create a new subdirectory under an existing dir-album's directory and
/// register it as a new album. Returns the new album's ID.
#[utoipa::path(
        post,
        path = "/post/create_dir_album",
        request_body = CreateDirAlbumData,
        responses(
            (status = 200, description = "New album ID", body = String),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[post("/post/create_dir_album", format = "json", data = "<json_data>")]
pub async fn create_dir_album(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    json_data: Json<CreateDirAlbumData>,
) -> AppResult<String> {
    let _ = auth?;
    let _ = read_only_mode?;

    let data = json_data.into_inner();
    let parent_album_id = data.parent_album_id;

    let name = data.name.trim().to_string();
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(AppError::new(ErrorKind::InvalidInput, "Invalid album name"));
    }

    let parent_dir = get_dir_path_for_album(parent_album_id)
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "Parent album not found"))?;

    let new_dir = parent_dir.join(&name);

    let new_album_id = tokio::task::spawn_blocking(move || -> Result<ArrayString<64>, AppError> {
        fs::create_dir(&new_dir).map_err(|e| {
            AppError::new(
                ErrorKind::Internal,
                format!("Failed to create directory: {e}"),
            )
        })?;

        let album_id = get_or_create_dir_album(new_dir).map_err(|e| {
            AppError::new(
                ErrorKind::Internal,
                format!("Failed to register album: {e}"),
            )
        })?;

        mark_album_for_update(parent_album_id);

        Ok(album_id)
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Task failed"))??;

    INDEX_COORDINATOR
        .execute_waiting(AlbumSelfUpdateTask::new(parent_album_id))
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to update parent album"))??;

    Ok(new_album_id.to_string())
}
