// src/router/delete/mod.rs
use rocket::Route;

pub fn generate_delete_routes() -> Vec<Route> {
    routes![delete_data, delete_invite, delete_user_handler]
}

// src/router/delete/delete_data.rs
use crate::error::{AppError, ErrorKind, ResultExt};
use crate::model::abstract_data::AbstractData;
use crate::openapi_components::Unauthorized;
use crate::process::dir_album::evict_dir_album;
use crate::router::auth::GuardAdmin;
use crate::router::auth::GuardMutate;
use crate::router::auth::GuardReauth;
use crate::router::auth::GuardUser;
use crate::router::{AppResult, GuardResult};
use crate::storage::db::{METADATA_TABLE, TREE};
use crate::tasks::BATCH_COORDINATOR;
use crate::tasks::batcher::flush_tree::FlushTreeTask;
use crate::tasks::queue::submit_interactive_album;
use anyhow::Result;
use arrayvec::ArrayString;
use futures::future::try_join_all;
use log::warn;
use redb::ReadableDatabase;
use rocket::serde::{Deserialize, Serialize, json::Json};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct DeleteList {
    /// Asset IDs to delete. Each asset is resolved via `ASSET_BY_ID` by its
    /// `asset_id` key. The asset file and sidecar are removed from disk.
    asset_ids: Vec<String>,
    timestamp: i64,
}

type DeleteResult = (
    Vec<AbstractData>,
    Vec<ArrayString<64>>,
    crate::tasks::path_epoch::PathLease,
);

/// Delete the listed assets: files, sidecars and metadata rows are removed.
///
/// `assetIds` are resolved by asset ID; each entry's file and `.xmp` sidecar
/// are removed from disk and its metadata row is dropped. Every album the
/// deletion touched is refreshed before the response returns.
///
/// Corner cases: An empty `assetIds` is a 400. IDs are processed in order, so
/// a malformed or unresolvable ID aborts the request part-way and files
/// already removed by earlier entries stay deleted. Deleting an album also
/// removes every descendant asset, sidecar and directory below it, and a
/// compressed thumbnail is kept while another asset still references the same
/// content hash.
///
/// Errors: 400 empty `assetIds`, or an ID that exceeds 64 bytes — 401 missing
/// or invalid credentials — 404 an ID names no asset — 405 read-only mode —
/// 500 storage failure.
#[utoipa::path(
        tag = "assets",
        request_body = DeleteList,
        security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardUser", "GuardMutate"],
            "credentials": ["session"],
            "admin": "any",
            "shareScope": "n-a",
            "effects": ["write"],
            "invariants": ["AUTH-12"]
        }))),
        responses(
            (status = 200, description = "Data deleted"),
            (status = 400, description = "Invalid input"),
            (status = 422, description = "Unprocessable entity"),
            (status = 401, response = Unauthorized),
            (status = 403, description = "Forbidden"),
            (status = 404, description = "Asset not found"),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[delete("/delete/delete-data", format = "json", data = "<json_data>")]
pub async fn delete_data(
    auth: GuardResult<GuardUser>,
    mutation: GuardResult<GuardMutate>,
    json_data: Json<DeleteList>,
) -> AppResult<()> {
    let GuardUser { claims } = auth?;
    let _ = mutation?;
    let (caller, _) = crate::router::put::users_password::caller_identity(&claims);

    if json_data.asset_ids.is_empty() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "assetIds must not be empty",
        ));
    }

    let (abstract_data_to_remove, all_affected_album_ids, _lease) = tokio::task::spawn_blocking({
        let asset_ids = json_data.asset_ids.clone();
        let timestamp = json_data.timestamp;
        let caller = caller.clone();
        move || process_deletes(&asset_ids, timestamp, &caller)
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;

    // Recursive album cleanup: for each album being removed, clean up
    // descendant files, sidecars, asset tables, and directories.
    // This runs after process_deletes validates all entries.
    tokio::task::spawn_blocking({
        let to_remove = abstract_data_to_remove.clone();
        move || cleanup_album_descendants(&to_remove)
    })
    .await
    .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))?;

    if !abstract_data_to_remove.is_empty() {
        BATCH_COORDINATOR
            .execute_batch_waiting(FlushTreeTask::remove(abstract_data_to_remove))
            .await
            .or_raise(|| (ErrorKind::Internal, "Failed to execute flush tree task"))?;
    }

    crate::storage::tree_view::tree_changed();

    try_join_all(
        all_affected_album_ids
            .into_iter()
            .map(|album_id| async move {
                submit_interactive_album(album_id).await.map_err(|e| {
                    AppError::new(
                        ErrorKind::Internal,
                        format!("Failed to update affected album: {e:#}"),
                    )
                })
            }),
    )
    .await?;
    Ok(())
}

/// Recursively clean up descendant assets for any albums in the removal list.
/// Removes files, sidecars, asset table entries, and directories from disk.
fn cleanup_album_descendants(abstract_data_to_remove: &[AbstractData]) {
    for abstract_data in abstract_data_to_remove {
        if let AbstractData::Album(alb) = abstract_data {
            if alb.metadata.dir_path.is_empty() {
                continue;
            }
            let dir_path = Path::new(&alb.metadata.dir_path);

            // Find and remove all descendant assets.
            if let Ok(descendants) =
                crate::storage::asset_store::get_assets_under_path(&alb.metadata.dir_path)
            {
                for desc in &descendants {
                    // Delete file + sidecar from disk.
                    let file_path = Path::new(&desc.path);
                    if let Err(e) = std::fs::remove_file(file_path)
                        && e.kind() != std::io::ErrorKind::NotFound
                    {
                        warn!("Failed to delete descendant {}: {e}", file_path.display());
                    }
                    let sidecar = file_path.with_extension("xmp");
                    if sidecar.exists()
                        && let Err(e) = std::fs::remove_file(&sidecar)
                    {
                        warn!(
                            "Failed to delete descendant sidecar {}: {e}",
                            sidecar.display()
                        );
                    }

                    // Clean up asset tables.
                    let _ = crate::storage::asset_store::remove_asset(desc);

                    // Evict child album caches.
                    if desc.kind == crate::model::asset::AssetKind::Album {
                        evict_dir_album(Path::new(&desc.path));
                    }
                }

                // Flush descendant records from METADATA_TABLE.
                let desc_abstract: Vec<AbstractData> = descendants
                    .iter()
                    .map(crate::process::transitor::asset_record_to_abstract_data)
                    .collect();
                if !desc_abstract.is_empty() {
                    // Use blocking wait to ensure METADATA_TABLE is updated before we proceed.
                    let _ = futures::executor::block_on(
                        BATCH_COORDINATOR
                            .execute_batch_waiting(FlushTreeTask::remove(desc_abstract)),
                    );
                }
            }

            // Remove the directory tree itself.
            let _ = std::fs::remove_dir_all(dir_path);
        }
    }
}

/// Bump and lease every path this delete request touches. Media records take
/// exact path leases; album records take a subtree lease because their
/// descendants are removed by directory.
fn lease_deleted_paths(
    collected: &[(
        crate::model::abstract_data::AbstractData,
        Vec<ArrayString<64>>,
    )],
) -> crate::tasks::path_epoch::PathLease {
    let mut exact = Vec::new();
    let mut subtrees = Vec::new();
    for (data, _) in collected {
        match data {
            crate::model::abstract_data::AbstractData::Album(album)
                if !album.metadata.dir_path.is_empty() =>
            {
                subtrees.push(std::path::PathBuf::from(&album.metadata.dir_path));
            }
            _ => {
                if let Some(entry) = data.path() {
                    exact.push(std::path::PathBuf::from(&entry.file));
                }
            }
        }
    }
    let lease =
        crate::tasks::path_epoch::lease_all(exact.iter().cloned(), subtrees.iter().cloned());
    for path in exact.iter().chain(subtrees.iter()) {
        crate::tasks::path_epoch::bump(path);
    }
    lease
}

/// Process deletions by asset ID.
///
/// For each `asset_id`:
/// 1. Compose the view from the asset's `AssetRecord` (+ stored payload).
/// 2. Delete the asset file + sidecar from disk.
/// 3. Remove the compressed thumbnail only if no other assets share the
///    same content hash (checked via `DUPE_INDEX`).
/// 4. Collect affected album IDs for later self-update.
fn process_deletes(
    asset_ids: &[String],
    _timestamp: i64,
    caller: &str,
) -> Result<DeleteResult, AppError> {
    use crate::model::metadata_record::compose_abstract_data;

    let txn = TREE
        .in_disk
        .begin_read()
        .or_raise(|| (ErrorKind::Database, "Failed to begin read transaction"))?;
    let metadata_table = txn
        .open_table(METADATA_TABLE)
        .or_raise(|| (ErrorKind::Database, "Failed to open METADATA_TABLE"))?;
    let id_table = txn
        .open_table(crate::storage::db::ASSET_BY_ID)
        .or_raise(|| (ErrorKind::Database, "Failed to open ASSET_BY_ID"))?;

    let mut all_affected_album_ids = Vec::new();
    let mut abstract_data_to_remove = Vec::new();

    // First pass: resolve every entry and enforce the write gate before any
    // filesystem mutation, so a forbidden entry aborts the whole request with
    // nothing deleted.
    let mut collected: Vec<(AbstractData, Vec<ArrayString<64>>)> = Vec::new();
    for asset_id_str in asset_ids {
        let asset_id: ArrayString<64> = ArrayString::from(asset_id_str.as_str()).map_err(|_| {
            AppError::new(
                ErrorKind::InvalidInput,
                format!("Invalid asset_id format: {asset_id_str}"),
            )
        })?;

        let record_json = id_table
            .get(&*asset_id)
            .or_raise(|| {
                (
                    ErrorKind::Database,
                    format!("Failed to look up asset {asset_id}"),
                )
            })?
            .ok_or_else(|| {
                AppError::new(ErrorKind::NotFound, format!("Asset not found: {asset_id}"))
            })?;
        let record: crate::model::asset::AssetRecord = serde_json::from_str(record_json.value())
            .or_raise(|| {
                (
                    ErrorKind::Database,
                    format!("Failed to deserialize AssetRecord for {asset_id}"),
                )
            })?;
        let payload = metadata_table
            .get(&*asset_id)
            .or_raise(|| {
                (
                    ErrorKind::Database,
                    format!("Failed to look up metadata for {asset_id}"),
                )
            })?
            .map(|guard| guard.value());
        let abstract_data = compose_abstract_data(&record, payload.as_ref());

        // Write gate: source must be writable by caller.
        if let Some(path) = abstract_data.visibility_path() {
            crate::storage::files::write_gate(caller, Some(&path), None)
                .map_err(|_| AppError::new(ErrorKind::PermissionDenied, "Asset not writable"))?;
        }

        // Collect affected albums.
        let affected_albums: Vec<ArrayString<64>> = match &abstract_data {
            AbstractData::Image(img) => img.metadata.album.iter().copied().collect(),
            AbstractData::Video(vid) => vid.metadata.album.iter().copied().collect(),
            AbstractData::Album(alb) => {
                if !alb.metadata.dir_path.is_empty() {
                    evict_dir_album(Path::new(&alb.metadata.dir_path));
                }
                vec![alb.object.id]
            }
        };

        collected.push((abstract_data, affected_albums));
    }

    // Lease every resolved path across the filesystem + row transaction so a
    // concurrent watcher removal or sweep verify skips instead of pruning or
    // detaching a row this request owns.
    let lease = lease_deleted_paths(&collected);

    // Second pass: perform deletions now that all entries validated.
    for (abstract_data, affected_albums) in collected {
        // Delete asset file + sidecar from disk.
        if let Some(file_entry) = abstract_data.path() {
            let original = Path::new(&file_entry.file);
            if let Err(e) = std::fs::remove_file(original)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                warn!("Failed to delete file {}: {e}", original.display());
            }
            let sidecar = original.with_extension("xmp");
            if sidecar.exists()
                && let Err(e) = std::fs::remove_file(&sidecar)
            {
                warn!("Failed to delete sidecar {}: {e}", sidecar.display());
            }
        }

        // Only remove thumbnail if no other assets share this content hash.
        let content_hash = abstract_data.hash();
        let other_refs = match crate::storage::asset_store::get_dupe_ids(&content_hash) {
            Ok(ids) => ids.len(),
            Err(_) => 0,
        };
        if other_refs <= 1 {
            let thumb = abstract_data.compressed_path();
            if thumb.exists()
                && let Err(e) = std::fs::remove_file(&thumb)
            {
                warn!("Failed to delete thumbnail {}: {e}", thumb.display());
            }
        }

        all_affected_album_ids.extend(affected_albums);
        abstract_data_to_remove.push(abstract_data);
    }

    Ok((abstract_data_to_remove, all_affected_album_ids, lease))
}

/// Revoke an invite token.
///
/// Admin-only. The invite's row is removed, so its token stops redeeming
/// immediately; a registration attempt afterwards answers 404. Ids come
/// from `GET /get/invites`.
///
/// Corner cases: Revoking an already-removed id is a 404, not a success —
/// the second caller cannot distinguish "revoked twice" from "never
/// existed", which leaks nothing.
///
/// Errors: 401 missing or invalid admin credentials — 404 unknown invite
/// id — 405 read-only mode — 500 storage failure.
#[utoipa::path(
        tag = "auth",
        params(
            ("invite_id" = &str, Path, description = "Invite id as listed by GET /get/invites"),
        ),
        security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardAdmin", "GuardMutate"],
            "credentials": ["session"],
            "admin": "admin",
            "shareScope": "n-a",
            "effects": ["manage"],
            "invariants": ["AUTH-11", "AUTH-12"]
        }))),
        responses(
            (status = 200, description = "Invite revoked"),
            (status = 401, response = Unauthorized),
            (status = 404, description = "Unknown invite id"),
            (status = 405, description = "Read-only mode"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[delete("/delete/invites/<invite_id>")]
pub fn delete_invite(
    auth: GuardResult<GuardAdmin>,
    mutation: GuardResult<GuardMutate>,
    invite_id: &str,
) -> AppResult<()> {
    let _ = auth?;
    let _ = mutation?;
    if !crate::auth::invites::revoke_invite(invite_id)? {
        return Err(AppError::new(
            ErrorKind::NotFound,
            format!("unknown invite: {invite_id}"),
        ));
    }
    Ok(())
}

/// Delete a user account (admin remediation).
///
/// Admin-only: a non-admin, share, or missing credential is denied at
/// dispatch with 401. The deletion refuses to leave zero admins; deleting
/// the sole admin is therefore impossible, while self-delete with another
/// admin present succeeds. The identity record (including its password
/// hash) is removed in one store write.
///
/// Requires re-authentication via the `X-Picasu-Password` header (403 when
/// it is missing or wrong). Refuses with 409 while the user still has
/// personal photos.
///
/// Corner cases: Unknown `user_id` names no user. An invalid id (empty or
/// over 64 chars) is a 400. Read-only mode refuses with 405.
///
/// Errors: 400 invalid `user_id` — 401 missing or invalid credentials,
/// non-admin caller, or share credentials — 403 missing or wrong
/// re-authentication password — 404 unknown `user_id` — 405 read-only mode
/// — 409 deletion would leave zero admins, or the user still has personal
/// photos — 500 storage failure.
#[utoipa::path(
        tag = "auth",
        params(
            ("user_id" = &str, Path, description = "User id to delete"),
        ),
        security(("cookie_jwt" = [])),
        extensions(("x-picasu-auth" = json!({
            "guards": ["GuardAdmin", "GuardMutate", "GuardReauth"],
            "credentials": ["session"],
            "admin": "admin",
            "shareScope": "n-a",
            "effects": ["manage"],
            "invariants": ["AUTH-11", "AUTH-12"]
        }))),
        responses(
            (status = 200, description = "User deleted"),
            (status = 400, description = "Invalid input"),
            (status = 401, response = Unauthorized),
            (status = 403, description = "Forbidden"),
            (status = 404, description = "Unknown user id"),
            (status = 405, description = "Read-only mode"),
            (status = 409, description = "Deletion would leave zero admins or user still has personal photos"),
            (status = 500, description = "Internal error"),
        )
    )
]
#[delete("/delete/users/<user_id>")]
pub async fn delete_user_handler(
    auth: GuardResult<GuardAdmin>,
    mutation: GuardResult<GuardMutate>,
    reauth: GuardResult<GuardReauth>,
    user_id: &str,
) -> AppResult<()> {
    let _ = auth?;
    let _ = mutation?;
    let _ = reauth?;
    let user_id = user_id.to_string();
    tokio::task::spawn_blocking(move || crate::auth::users::delete_user(&user_id))
        .await
        .or_raise(|| (ErrorKind::Internal, "Failed to join blocking task"))??;
    crate::storage::tree_view::tree_changed();
    Ok(())
}

#[cfg(test)]
mod tests {
    use rocket::http::Status;

    use crate::tests::bootstrap::{cookie, lock, seed, setup, setup_image_home, write_config};
    use serde_json::json;

    fn del(
        client: &rocket::local::blocking::Client,
        cookie: Option<rocket::http::Cookie<'static>>,
        user_id: &str,
    ) -> Status {
        let mut req = client.delete(format!("/delete/users/{user_id}"));
        if let Some(c) = cookie {
            // GuardReauth requires the caller's password in X-Picasu-Password;
            // mint it from the seeded default password for that caller.
            let pw = match crate::tests::bootstrap::decode(c.value()).role {
                crate::router::auth::Role::User { id, .. } => {
                    Some(crate::tests::fixtures::test_password(&id))
                }
                crate::router::auth::Role::Share(_) => None,
            };
            req = req.cookie(c);
            if let Some(p) = pw {
                req = req.header(rocket::http::Header::new("X-Picasu-Password", p));
            }
        }
        req.dispatch().status()
    }

    #[test]
    fn delete_ok_is_200() {
        let _g = lock();
        let client = setup();
        seed("d-root", true, None);
        seed("d-bob", false, None);
        let status = del(&client, Some(cookie("d-root", true)), "d-bob");
        assert_eq!(status, Status::Ok);
        assert!(
            crate::auth::users::user_record("d-bob")
                .expect("get")
                .is_none()
        );
    }

    #[test]
    fn delete_unknown_is_404() {
        let _g = lock();
        let client = setup();
        seed("d-root", true, None);
        let status = del(&client, Some(cookie("d-root", true)), "d-ghost");
        assert_eq!(status, Status::NotFound);
    }

    #[test]
    fn delete_sole_admin_self_is_409() {
        let _g = lock();
        let client = setup();
        seed("d-sole", true, None);
        let status = del(&client, Some(cookie("d-sole", true)), "d-sole");
        assert_eq!(status, Status::Conflict);
        assert!(
            crate::auth::users::user_record("d-sole")
                .expect("get")
                .is_some()
        );
    }

    #[test]
    fn delete_sole_admin_while_others_are_users_is_409() {
        let _g = lock();
        let client = setup();
        seed("d-admin", true, None);
        seed("d-user", false, None);
        let status = del(&client, Some(cookie("d-admin", true)), "d-admin");
        assert_eq!(status, Status::Conflict);
    }

    #[test]
    fn delete_non_admin_caller_is_401() {
        let _g = lock();
        let client = setup();
        seed("d-root", true, None);
        seed("d-bob", false, None);
        let status = del(&client, Some(cookie("d-bob", false)), "d-bob");
        assert_eq!(status, Status::Unauthorized);
    }

    #[test]
    fn delete_unauthenticated_is_401() {
        let _g = lock();
        let client = setup();
        seed("d-root", true, None);
        let status = del(&client, None, "d-root");
        assert_eq!(status, Status::Unauthorized);
    }

    #[test]
    fn delete_read_only_is_405() {
        let _g = lock();
        let client = setup();
        seed("d-root", true, None);
        seed("d-bob", false, None);
        write_config(&json!({ "read_only_mode": true }));
        let status = del(&client, Some(cookie("d-root", true)), "d-bob");
        assert_eq!(status, Status::MethodNotAllowed);
    }

    #[test]
    fn delete_bad_id_is_400() {
        let _g = lock();
        let client = setup();
        seed("d-root", true, None);
        let status = del(&client, Some(cookie("d-root", true)), "   ");
        assert_eq!(status, Status::BadRequest);
    }

    #[test]
    fn delete_removes_store_entry_and_login_fails() {
        let _g = lock();
        let client = setup();
        seed("d-root", true, None);
        seed("d-victim", false, None);
        let pw = crate::tests::fixtures::test_password("d-victim");
        let status = del(&client, Some(cookie("d-root", true)), "d-victim");
        assert_eq!(status, Status::Ok);
        // Store entry gone.
        assert!(
            crate::auth::user_store::read(&crate::auth::users::user_store_path(), "d-victim")
                .is_none()
        );
        // Login with deleted credentials fails.
        let login_status = crate::tests::bootstrap::login_status(&client, "d-victim", &pw);
        assert_eq!(login_status, Status::Unauthorized);
    }

    #[test]
    fn delete_self_with_other_admin_present_succeeds() {
        let _g = lock();
        let client = setup();
        seed("d-a", true, None);
        seed("d-b", true, None);
        let status = del(&client, Some(cookie("d-a", true)), "d-a");
        assert_eq!(status, Status::Ok);
    }

    fn enable_users_home() -> std::path::PathBuf {
        let data_path = crate::storage::files::DATA_PATH
            .get()
            .expect("DATA_PATH set");
        let users_home = data_path.join("users");
        let _ = std::fs::remove_dir_all(&users_home);
        std::fs::create_dir_all(&users_home).expect("create users home");
        let mut c = crate::model::config::APP_CONFIG
            .get()
            .expect("cfg")
            .write()
            .expect("lock");
        c.users_home = Some(users_home.clone());
        users_home
    }

    fn personal_album_id(users_home: &std::path::Path, user: &str) -> String {
        let dir = users_home.join(user);
        crate::process::dir_album::get_album_id_for_dir(&dir)
            .expect("personal home album id")
            .to_string()
    }

    fn create_share_for(
        client: &rocket::local::blocking::Client,
        creator: &str,
        creator_admin: bool,
        album_id: &str,
    ) -> String {
        let resp = client
            .post("/post/create_share")
            .cookie(cookie(creator, creator_admin))
            .header(rocket::http::ContentType::JSON)
            .body(
                serde_json::json!({
                    "albumId": album_id,
                    "description": "owned",
                    "password": null,
                    "showMetadata": true,
                    "showDownload": true,
                    "showUpload": false,
                    "exp": 0
                })
                .to_string(),
            )
            .dispatch();
        assert_eq!(resp.status(), Status::Ok, "create_share as {creator}");
        let body: serde_json::Value = resp.into_json().expect("create_share json");
        body["link"].as_str().expect("link").trim().to_string()
    }

    fn anon_prefetch(
        client: &rocket::local::blocking::Client,
        album_id: &str,
        share_id: &str,
    ) -> Status {
        client
            .post("/get/prefetch")
            .header(rocket::http::Header::new(
                "x-album-id",
                album_id.to_string(),
            ))
            .header(rocket::http::Header::new(
                "x-share-id",
                share_id.to_string(),
            ))
            .header(rocket::http::ContentType::JSON)
            .dispatch()
            .status()
    }

    #[test]
    fn delete_with_personal_photos_is_409_and_unchanged() {
        let _g = lock();
        let client = setup_image_home();
        let users_home = enable_users_home();
        seed("d-root", true, None);
        seed("d-victim", false, None);
        let dir = users_home.join("d-victim");
        std::fs::create_dir_all(&dir).expect("user dir");
        std::fs::write(dir.join("photo.jpg"), [0xff, 0xd8, 0xff, 0xe0]).expect("photo");

        let (album_id, _) =
            crate::tests::fixtures::authz::create_album(&client, "/dphotos/photo.jpg");
        let share_id = create_share_for(&client, "d-victim", false, &album_id);

        let status = del(&client, Some(cookie("d-root", true)), "d-victim");
        assert_eq!(status, Status::Conflict);

        // Everything intact.
        assert!(
            crate::auth::users::user_record("d-victim")
                .expect("get")
                .is_some()
        );
        assert!(dir.join("photo.jpg").exists());
        assert_eq!(anon_prefetch(&client, &album_id, &share_id), Status::Ok);
    }

    #[test]
    fn delete_without_photos_cascades_shares_and_prunes_album() {
        let _g = lock();
        let client = setup_image_home();
        let users_home = enable_users_home();
        seed("d-root", true, None);
        seed("d-victim", false, None);
        let personal_dir = users_home.join("d-victim");
        std::fs::create_dir_all(&personal_dir).expect("user dir");

        // share on the victim's own personal album
        let personal_album = personal_album_id(&users_home, "d-victim");
        let share_own = create_share_for(&client, "d-victim", false, &personal_album);

        // share on a public album
        let (public_album, _) =
            crate::tests::fixtures::authz::create_album(&client, "/dpub/photo.jpg");
        let share_pub = create_share_for(&client, "d-victim", false, &public_album);

        let status = del(&client, Some(cookie("d-root", true)), "d-victim");
        assert_eq!(status, Status::Ok);

        assert!(
            crate::auth::users::user_record("d-victim")
                .expect("get")
                .is_none()
        );
        assert_eq!(
            anon_prefetch(&client, &personal_album, &share_own),
            Status::NotFound
        );
        assert_eq!(
            anon_prefetch(&client, &public_album, &share_pub),
            Status::NotFound
        );
        assert!(
            crate::process::dir_album::get_album_id_for_dir(&personal_dir).is_none(),
            "personal album record should be pruned"
        );
    }

    #[test]
    fn delete_missing_confirmation_password_is_403() {
        let _g = lock();
        let client = setup();
        seed("d-root", true, None);
        seed("d-victim", false, None);
        let status = client
            .delete("/delete/users/d-victim")
            .cookie(cookie("d-root", true))
            .dispatch()
            .status();
        assert_eq!(status, Status::Forbidden);
        assert!(
            crate::auth::users::user_record("d-victim")
                .expect("get")
                .is_some()
        );
    }

    #[test]
    fn delete_wrong_confirmation_password_is_403() {
        let _g = lock();
        let client = setup();
        seed("d-root", true, None);
        seed("d-victim", false, None);
        let status = client
            .delete("/delete/users/d-victim")
            .cookie(cookie("d-root", true))
            .header(rocket::http::Header::new(
                "X-Picasu-Password",
                "definitely-wrong",
            ))
            .dispatch()
            .status();
        assert_eq!(status, Status::Forbidden);
        assert!(
            crate::auth::users::user_record("d-victim")
                .expect("get")
                .is_some()
        );
    }

    #[test]
    fn delete_after_manual_file_removal_succeeds() {
        let _g = lock();
        let client = setup_image_home();
        let users_home = enable_users_home();
        seed("d-root", true, None);
        seed("d-victim", false, None);
        let dir = users_home.join("d-victim");
        std::fs::create_dir_all(&dir).expect("user dir");
        let photo = dir.join("photo.jpg");
        std::fs::write(&photo, [0xff, 0xd8, 0xff, 0xe0]).expect("photo");
        // simulate operator removal
        std::fs::remove_file(&photo).expect("remove");
        let status = del(&client, Some(cookie("d-root", true)), "d-victim");
        assert_eq!(status, Status::Ok);
        assert!(
            crate::auth::users::user_record("d-victim")
                .expect("get")
                .is_none()
        );
    }
}
