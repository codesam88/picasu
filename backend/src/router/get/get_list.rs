use crate::error::AppError;
use crate::model::album::Share;
use crate::openapi_components::Unauthorized;
use crate::process::dir_album::get_parent_album_id;
use crate::router::auth::GuardUser;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::TREE;
use crate::storage::db::TagInfo;
use arrayvec::ArrayString;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// List every tag in the tree with the number of assets carrying it.
///
/// Counts every stored tag in the tree together with how many assets carry it,
/// so the result covers assets outside the snapshot or share a client happens
/// to be viewing.
///
/// Corner cases: the counts are accumulated in parallel, so the order of the
/// returned list is unspecified and not stable between calls.
///
/// Errors: 400 invalid input — 401 missing, invalid or share-role session —
/// 403 no enrolled users — 500 storage failure.
#[utoipa::path(
        tag = "timeline",
        security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardUser"],
            "credentials": ["session"],
            "admin": "any",
            "shareScope": "n-a",
            "effects": ["read"],
            "invariants": []
        }))),
        responses(
            (status = 200, description = "List of tags", body = Vec<TagInfo>),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 403, description = "Forbidden"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[get("/get/get-tags")]
pub async fn get_tags(auth: GuardResult<GuardUser>) -> AppResult<Json<Vec<TagInfo>>> {
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
    #[schema(value_type = HashMap<String, ShareInfo>)]
    pub share_list: HashMap<ArrayString<64>, ShareInfo>,
    pub dir_path: Option<String>,
    /// Album ID of the direct parent directory album, or `None` for top-level
    /// dir albums and all user-created albums.
    pub parent_album_id: Option<String>,
}

/// One album's share entry as served to clients: the policy plus presence of
/// a password, never the secret itself.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
pub struct ShareInfo {
    #[schema(value_type = String)]
    pub url: ArrayString<64>,
    pub description: String,
    /// Whether a share password is configured; the stored secret never
    /// leaves the backend through an album read.
    pub has_password: bool,
    pub show_metadata: bool,
    pub show_download: bool,
    pub show_upload: bool,
    pub exp: i64,
}

impl From<&Share> for ShareInfo {
    fn from(share: &Share) -> Self {
        Self {
            url: share.url,
            description: share.description.clone(),
            has_password: share.password.is_some(),
            show_metadata: share.show_metadata,
            show_download: share.show_download,
            show_upload: share.show_upload,
            exp: share.exp,
        }
    }
}

/// List every album the metadata tree knows.
///
/// Returns one entry per album with `albumId`, `albumName`, the album's
/// `shareList` share definitions, `dirPath` and `parentAlbumId`.
///
/// Corner cases: `dirPath` is reported relative to the configured `imagePath`
/// so clients never handle the absolute library location, falling back to the
/// absolute path for albums outside that root; `parentAlbumId` names the direct
/// parent directory album and is `null` for top-level dir albums and all
/// manually created albums. Share entries carry `hasPassword` presence only —
/// the stored password is never returned by an album read.
///
/// Errors: 401 missing, invalid or share-role session — 403 no enrolled
/// users — 500 storage failure.
#[utoipa::path(
        tag = "albums",
        security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardUser"],
            "credentials": ["session"],
            "admin": "any",
            "shareScope": "n-a",
            "effects": ["read"],
            "invariants": []
        }))),
        responses(
            (status = 200, description = "List of albums", body = Vec<AlbumInfo>),
            (status = 401, response = Unauthorized),
            (status = 403, description = "Forbidden"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[get("/get/get-albums")]
pub async fn get_albums(auth: GuardResult<GuardUser>) -> AppResult<Json<Vec<AlbumInfo>>> {
    let _ = auth?;
    tokio::task::spawn_blocking(move || {
        let image_home = crate::model::config::with_config(|c| c.image_home.clone());

        let album_list = TREE
            .read_albums()
            .map_err(|e| e.context("Failed to read albums"))?;
        let album_info_list = album_list
            .into_iter()
            .map(|album| {
                // parent_album_id lookup uses the raw absolute path stored in the DB
                let parent_album_id = get_parent_album_id(Path::new(&album.metadata.dir_path))
                    .map(|id| id.to_string());
                // Expose dir_path as a path relative to IMAGE_HOME so the frontend
                // never needs to know or handle the absolute image library location.
                let dir_path = Some({
                    let dir = &album.metadata.dir_path;
                    image_home
                        .as_ref()
                        .and_then(|root| Path::new(dir).strip_prefix(root).ok())
                        .map_or_else(|| dir.clone(), |rel| rel.to_string_lossy().into_owned())
                });
                AlbumInfo {
                    album_id: album.object.id.to_string(),
                    album_name: album.metadata.title,
                    share_list: album
                        .metadata
                        .share_list
                        .iter()
                        .map(|(id, share)| (*id, ShareInfo::from(share)))
                        .collect(),
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
