use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::album::Share;
use crate::model::metadata_record::MetadataRecord;
use crate::openapi_components::Unauthorized;
use crate::router::GuardResult;
use crate::router::auth::GuardAuth;
use crate::router::auth::GuardReadOnlyMode;
use crate::storage::db::TREE;
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::update_tree::UpdateTreeTask;
use crate::{router::AppResult, storage::db::METADATA_TABLE};

use arrayvec::ArrayString;
use redb::ReadableTable;
use rocket::serde::{Deserialize, Serialize, json::Json};
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct EditShare {
    #[schema(value_type = String)]
    album_id: ArrayString<64>,
    share: Share,
}

/// Create or replace one of an album's share entries.
///
/// The submitted `share` object is stored under the key `share.url`, replacing
/// any entry already held under that key, so a share is edited by posting a
/// complete object. The password is the exception to the full replace: an
/// absent or `null` `password` keeps the stored secret, the empty string
/// clears it, and any other string sets it. The payload is never checked
/// against the album, no `.albuminfo.xmp` sidecar is written, and the
/// in-memory tree is rebuilt before the call returns.
///
/// Corner cases: an `albumId` that holds no album row is a silent no-op that
/// still answers 200.
///
/// Errors: 400 malformed body — 401 missing or invalid admin credentials; share
/// tokens are not accepted — 405 read-only mode — 500 storage failure.
#[utoipa::path(
        tag = "albums",
        request_body = EditShare,
        security(("cookie_jwt" = [])),
        responses(
            (status = 200, description = "Share updated"),
            (status = 400, description = "Invalid input"),
            (status = 404, description = "Not found"),
            (status = 422, description = "Unprocessable entity"),
            (status = 401, response = Unauthorized),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[put("/put/edit_share", format = "json", data = "<json_data>")]
pub async fn edit_share(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    json_data: Json<EditShare>,
) -> AppResult<()> {
    let _ = auth?;
    let _ = read_only_mode?;
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let txn = TREE
            .in_disk
            .begin_write()
            .or_raise(|| (ErrorKind::Database, "Failed to begin transaction"))?;
        {
            let mut metadata_table = txn
                .open_table(METADATA_TABLE)
                .or_raise(|| (ErrorKind::Database, "Failed to open data table"))?;

            let album_opt = metadata_table
                .get(json_data.album_id.as_str())
                .or_raise(|| (ErrorKind::Database, "Failed to get album"))?
                .and_then(|guard| match guard.value() {
                    MetadataRecord::Album(album) => Some(album),
                    _ => None,
                });

            if let Some(mut album) = album_opt {
                // Password contract: absent or `null` keeps the stored
                // secret, the empty string clears it, any other string
                // replaces it. Album reads no longer disclose the value, so
                // a full-object replace without keep-semantics would silently
                // wipe every password on any other edit.
                let mut share = json_data.share.clone();
                match share.password.as_deref() {
                    Some("") => share.password = None,
                    None => {
                        share.password = album
                            .share_list
                            .get(&share.url)
                            .and_then(|existing| existing.password.clone());
                    }
                    Some(_) => {}
                }
                album.share_list.insert(share.url, share);
                metadata_table
                    .insert(json_data.album_id.as_str(), MetadataRecord::Album(album))
                    .or_raise(|| (ErrorKind::Database, "Failed to update album"))?;
            }
        }
        txn.commit()
            .or_raise(|| (ErrorKind::Database, "Failed to commit transaction"))?;
        Ok(())
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;
    BATCH_COORDINATOR
        .execute_batch_waiting(UpdateTreeTask)
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to update tree"))?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct DeleteShare {
    #[schema(value_type = String)]
    album_id: ArrayString<64>,
    #[schema(value_type = String)]
    share_id: ArrayString<64>,
}

/// Remove one share from an album.
///
/// `albumId` names the album and `shareId` the entry to drop from its share
/// list. The removal is committed to the album's stored share list, no
/// `.albuminfo.xmp` sidecar is written, and the in-memory tree is rebuilt
/// before the call returns.
///
/// Corner cases: removing a share that is not present, or addressing an
/// `albumId` that holds no album row, is a silent no-op that still answers 200
/// rather than a 404.
///
/// Errors: 400 malformed body — 401 missing or invalid admin credentials; share
/// tokens are not accepted — 405 read-only mode — 500 storage failure.
#[utoipa::path(
        tag = "albums",
        request_body = DeleteShare,
        security(("cookie_jwt" = [])),
        responses(
            (status = 200, description = "Share deleted"),
            (status = 400, description = "Invalid input"),
            (status = 404, description = "Not found"),
            (status = 422, description = "Unprocessable entity"),
            (status = 401, response = Unauthorized),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[put("/put/delete_share", format = "json", data = "<json_data>")]
pub async fn delete_share(
    auth: GuardResult<GuardAuth>,
    read_only_mode: GuardResult<GuardReadOnlyMode>,
    json_data: Json<DeleteShare>,
) -> AppResult<()> {
    let _ = auth?;
    let _ = read_only_mode?;
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let txn = TREE
            .in_disk
            .begin_write()
            .or_raise(|| (ErrorKind::Database, "Failed to begin transaction"))?;
        {
            let mut metadata_table = txn
                .open_table(METADATA_TABLE)
                .or_raise(|| (ErrorKind::Database, "Failed to open data table"))?;

            let album_opt = metadata_table
                .get(json_data.album_id.as_str())
                .or_raise(|| (ErrorKind::Database, "Failed to get album"))?
                .and_then(|guard| match guard.value() {
                    MetadataRecord::Album(album) => Some(album),
                    _ => None,
                });

            if let Some(mut album) = album_opt {
                album.share_list.remove(&json_data.share_id);
                metadata_table
                    .insert(json_data.album_id.as_str(), MetadataRecord::Album(album))
                    .or_raise(|| (ErrorKind::Database, "Failed to update album"))?;
            }
        }
        txn.commit()
            .or_raise(|| (ErrorKind::Database, "Failed to commit transaction"))?;
        Ok(())
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;
    BATCH_COORDINATOR
        .execute_batch_waiting(UpdateTreeTask)
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to update tree"))?;
    Ok(())
}
