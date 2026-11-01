use arrayvec::ArrayString;

use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::metadata_record::MetadataRecord;
use crate::openapi_components::Unauthorized;
use crate::process::dir_album::get_dir_path_for_album;
use crate::router::auth::GuardUser;
use crate::router::put::users_password::caller_identity;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::{METADATA_TABLE, TREE};
use crate::storage::files::visible_to;
use log::info;
use redb::ReadableDatabase;
use rocket::get;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct ShareTokenReturn {
    pub token: String,
}

/// Reveal the bearer token for one of an album's shares.
///
/// The album must be visible to the caller (invisible albums answer 404,
/// same no-oracle canon as the other share endpoints). The entry is then
/// located by its non-secret fingerprint; the bearer is returned only when
/// the entry's `created_by` equals the caller. Any other case — unknown
/// fingerprint, foreign creator, including an admin who did not create the
/// share — answers 404 with the same message, so existence is never
/// disclosed to non-creators. Retrieval is logged at info level without
/// the token value.
#[utoipa::path(
        tag = "albums",
        params(
            ("album_id" = String, Query, description = "Album holding the share"),
            ("fingerprint" = String, Query, description = "16-char blake3 fingerprint of the share bearer"),
        ),
        security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardUser"],
            "credentials": ["session"],
            "admin": "any",
            "shareScope": "n-a",
            "effects": ["read"],
            "invariants": ["AUTH-11"]
        }))),
        responses(
            (status = 200, description = "Share bearer token", body = ShareTokenReturn),
            (status = 401, response = Unauthorized),
            (status = 403, description = "Forbidden"),
            (status = 404, description = "Not found"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[get("/get/share/token?<album_id>&<fingerprint>")]
// Rocket resolves query guards by value; the owned Strings are required.
#[allow(clippy::needless_pass_by_value)]
pub fn get_share_token(
    auth: GuardResult<GuardUser>,
    album_id: String,
    fingerprint: String,
) -> AppResult<Json<ShareTokenReturn>> {
    let auth = auth?;
    let (caller, _) = caller_identity(&auth.claims);

    let album_id = ArrayString::<64>::from(album_id.as_str())
        .map_err(|_| AppError::new(ErrorKind::NotFound, "Album not found"))?;
    let album_dir = get_dir_path_for_album(album_id)
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Album not found"))?;
    if !visible_to(&caller, &album_dir) {
        return Err(AppError::new(ErrorKind::NotFound, "Album not found"));
    }

    let read_txn = TREE
        .in_disk
        .begin_read()
        .or_raise(|| (ErrorKind::Database, "Failed to begin transaction"))?;
    let metadata_table = read_txn
        .open_table(METADATA_TABLE)
        .or_raise(|| (ErrorKind::Database, "Failed to open data table"))?;
    let album_opt = metadata_table
        .get(album_id.as_str())
        .or_raise(|| (ErrorKind::Database, "Failed to get album"))?
        .and_then(|guard| match guard.value() {
            MetadataRecord::Album(album) => Some(album),
            _ => None,
        });

    // Fingerprint match alone reveals nothing: the bearer leaves only when
    // the entry's creator equals the caller. A foreign match falls through
    // to the same 404 as an unknown fingerprint.
    let mut revealed: Option<String> = None;
    if let Some(album) = album_opt {
        for (token, share) in &album.share_list {
            if share.fingerprint() == fingerprint {
                if share.created_by == caller {
                    revealed = Some(token.as_str().to_string());
                }
                break;
            }
        }
    }

    match revealed {
        Some(token) => {
            info!("share token revealed: actor={caller} album={album_id}");
            Ok(Json(ShareTokenReturn { token }))
        }
        None => Err(AppError::new(ErrorKind::NotFound, "Share not found")),
    }
}
