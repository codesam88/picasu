use crate::error::AppError;
use crate::model::album::Share;
use crate::process::dir_album::get_parent_album_id;
use crate::router::auth::GuardAuth;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::TREE;
use crate::storage::db::TagInfo;
use arrayvec::ArrayString;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[utoipa::path(
        get,
        path = "/get/get-tags",
        responses(
            (status = 200, description = "List of tags", body = Vec<TagInfo>),
            (status = 400, description = "Invalid input"),
        )
    )
]
#[get("/get/get-tags")]
pub async fn get_tags(auth: GuardResult<GuardAuth>) -> AppResult<Json<Vec<TagInfo>>> {
    let _ = auth?;
    tokio::task::spawn_blocking(move || {
        let vec_tags_info = TREE.read_tags();
        Ok(Json(vec_tags_info))
    })
    .await
    .map_err(|e| AppError::from(anyhow::Error::from(e)))? // Handle JoinError
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlbumInfo {
    pub album_id: String,
    pub album_name: Option<String>,
    #[schema(value_type = HashMap<String, crate::model::album::Share>)]
    pub share_list: HashMap<ArrayString<64>, Share>,
    pub dir_path: Option<String>,
    /// Album ID of the direct parent directory album, or `None` for top-level
    /// dir albums and all user-created albums.
    pub parent_album_id: Option<String>,
}

#[utoipa::path(
        get,
        path = "/get/get-albums",
        responses(
            (status = 200, description = "List of albums", body = Vec<AlbumInfo>),
        )
    )
]
#[get("/get/get-albums")]
pub async fn get_albums(auth: GuardResult<GuardAuth>) -> AppResult<Json<Vec<AlbumInfo>>> {
    let _ = auth?;
    tokio::task::spawn_blocking(move || {
        let album_list = TREE
            .read_albums()
            .map_err(|e| e.context("Failed to read albums"))?;
        let album_info_list = album_list
            .into_iter()
            .map(|album| {
                // Resolve the absolute dir_path from namespace + relative for parent lookup
                let abs_dir_path = crate::process::namespace::namespace_resolve(
                    &album.metadata.namespace,
                    &album.metadata.dir_path,
                );
                let parent_album_id = abs_dir_path
                    .as_ref()
                    .and_then(|p| get_parent_album_id(p))
                    .map(|id| id.to_string());
                // dir_path is already relative within the namespace
                let dir_path = Some(album.metadata.dir_path.clone());
                AlbumInfo {
                    album_id: album.object.id.to_string(),
                    album_name: album.metadata.title,
                    share_list: album.metadata.share_list,
                    dir_path,
                    parent_album_id,
                }
            })
            .collect();
        Ok(Json(album_info_list))
    })
    .await
    .map_err(|e| AppError::from(anyhow::Error::from(e)))?
}
