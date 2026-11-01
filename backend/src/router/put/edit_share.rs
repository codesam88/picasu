use crate::auth::password::hash_share_password;
use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::album::Share;
use crate::model::metadata_record::MetadataRecord;
use crate::openapi_components::Unauthorized;
use crate::process::dir_album::get_dir_path_for_album;
use crate::router::GuardResult;
use crate::router::auth::GuardMutate;
use crate::router::auth::GuardUser;
use crate::router::put::users_password::caller_identity;
use crate::storage::db::TREE;
use crate::storage::files::visible_to;
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
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardUser", "GuardMutate"],
            "credentials": ["session"],
            "admin": "any",
            "shareScope": "n-a",
            "effects": ["write"],
            "invariants": ["AUTH-11", "AUTH-12"]
        }))),
        responses(
            (status = 200, description = "Share updated"),
            (status = 400, description = "Invalid input"),
            (status = 404, description = "Not found"),
            (status = 422, description = "Unprocessable entity"),
            (status = 401, response = Unauthorized),
            (status = 403, description = "Forbidden"),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[put("/put/edit_share", format = "json", data = "<json_data>")]
pub async fn edit_share(
    auth: GuardResult<GuardUser>,
    mutation: GuardResult<GuardMutate>,
    json_data: Json<EditShare>,
) -> AppResult<()> {
    let GuardUser { claims } = auth?;
    let _ = mutation?;
    let (caller, _) = caller_identity(&claims);
    // Ownership check on the target album before any write.
    let album_id_for_check = json_data.album_id;
    let album_dir = get_dir_path_for_album(album_id_for_check)
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Album not found"))?;
    if !visible_to(&caller, &album_dir) {
        return Err(AppError::new(ErrorKind::NotFound, "Album not found"));
    }
    let caller_for_task = caller.clone();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let json_data = json_data;
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
                // Ownership of an existing share: only its creator may edit it.
                let existing_created_by = album
                    .share_list
                    .get(&json_data.share.url)
                    .map(|s| s.created_by.clone());
                if let Some(cb) = existing_created_by
                    && cb != caller_for_task
                {
                    return Err(AppError::new(
                        ErrorKind::PermissionDenied,
                        "Share not owned by caller",
                    ));
                }
                // Password contract: absent or `null` keeps the stored
                // secret, the empty string clears it, any other string
                // replaces it. Album reads no longer disclose the value, so
                // a full-object replace without keep-semantics would silently
                // wipe every password on any other edit.
                let mut share = json_data.share.clone();
                // Snapshot the incoming secret before borrowing `share.password`.
                let incoming = share.password.clone();
                match incoming.as_deref() {
                    Some("") => share.password = None,
                    None => {
                        share.password = album
                            .share_list
                            .get(&share.url)
                            .and_then(|existing| existing.password.clone());
                    }
                    Some(pw) => share.password = Some(hash_share_password(pw)),
                }
                // Creator is server-assigned, never taken from the client:
                // keep the stored creator across the full-object replace,
                // falling back to the editor when the entry is absent.
                share.created_by = album.share_list.get(&share.url).map_or_else(
                    || caller_for_task.clone(),
                    |existing| existing.created_by.clone(),
                );
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
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardUser", "GuardMutate"],
            "credentials": ["session"],
            "admin": "any",
            "shareScope": "n-a",
            "effects": ["write"],
            "invariants": ["AUTH-11", "AUTH-12"]
        }))),
        responses(
            (status = 200, description = "Share deleted"),
            (status = 400, description = "Invalid input"),
            (status = 404, description = "Not found"),
            (status = 422, description = "Unprocessable entity"),
            (status = 401, response = Unauthorized),
            (status = 403, description = "Forbidden"),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[put("/put/delete_share", format = "json", data = "<json_data>")]
pub async fn delete_share(
    auth: GuardResult<GuardUser>,
    mutation: GuardResult<GuardMutate>,
    json_data: Json<DeleteShare>,
) -> AppResult<()> {
    let GuardUser { claims } = auth?;
    let _ = mutation?;
    let (caller, _) = caller_identity(&claims);
    // Ownership check on the target album before any write / existence handling.
    let album_id_for_check = json_data.album_id;
    let album_dir = get_dir_path_for_album(album_id_for_check)
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Album not found"))?;
    if !visible_to(&caller, &album_dir) {
        return Err(AppError::new(ErrorKind::NotFound, "Album not found"));
    }
    let caller_for_task = caller.clone();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let json_data = json_data;
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
                // Ownership of an existing share: only its creator may delete it.
                // Missing shares stay a silent no-op.
                if let Some(existing) = album.share_list.get(&json_data.share_id)
                    && existing.created_by != caller_for_task
                {
                    return Err(AppError::new(
                        ErrorKind::PermissionDenied,
                        "Share not owned by caller",
                    ));
                }
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
