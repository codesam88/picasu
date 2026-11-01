//! Per-user share ownership: album listings show only the shares the
//! caller created. The creator id is stamped server-side on create and
//! preserved across edits, so the filter reads it straight off the row.

#![cfg(test)]

use rocket::http::{ContentType, Status};
use serde_json::{Value, json};
use std::path::PathBuf;

use crate::tests::bootstrap::{cookie, lock, seed, setup_image_home};
use crate::tests::fixtures::authz::create_album;

fn share_body(album_id: &str) -> Value {
    json!({
        "albumId": album_id,
        "description": "owned",
        "password": null,
        "showMetadata": true,
        "showDownload": true,
        "showUpload": false,
        "exp": 0
    })
}

/// Create a share on `album_id` as `creator`, returning the share id.
fn create_share_as(
    client: &rocket::local::blocking::Client,
    creator: &str,
    creator_admin: bool,
    album_id: &str,
) -> String {
    let create = client
        .post("/post/create_share")
        .cookie(cookie(creator, creator_admin))
        .header(ContentType::JSON)
        .body(share_body(album_id).to_string())
        .dispatch();
    assert_eq!(create.status(), Status::Ok, "create_share as {creator}");
    create.into_string().expect("share id").trim().to_string()
}

/// Share urls the caller's `GET /get/get-albums` exposes for `album_id`.
fn listed_share_urls(
    client: &rocket::local::blocking::Client,
    user: &str,
    admin: bool,
    album_id: &str,
) -> Vec<String> {
    let albums: Value = client
        .get("/get/get-albums")
        .cookie(cookie(user, admin))
        .dispatch()
        .into_json()
        .expect("get-albums json");
    albums
        .as_array()
        .expect("album array")
        .iter()
        .find(|a| a["albumId"].as_str() == Some(album_id))
        .expect("album listed")["shareList"]
        .as_object()
        .expect("shareList object")
        .keys()
        .cloned()
        .collect()
}

/// Creator sees own share by fingerprint; admins see every share by
/// fingerprint only; no bearer value leaks into any listing.
#[test]
fn listings_scope_shares_to_their_creator() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/owned/photo.jpg");
    seed("own-root", true, None);
    seed("own-other", true, None);

    let share_id = create_share_as(&client, "own-root", true, &album_id);
    let fp = share_fingerprint_for(&share_id);

    // Creator sees own share, keyed by fingerprint, never by bearer.
    let root_keys = listed_share_urls(&client, "own-root", true, &album_id);
    assert!(
        root_keys.contains(&fp),
        "creator still sees own share by fingerprint, got {root_keys:?}"
    );
    assert!(
        !root_keys.contains(&share_id),
        "creator listing must not be keyed by the bearer, got {root_keys:?}"
    );

    // The ShareInfo entry carries the fingerprint and no url/bearer field.
    let albums: Value = client
        .get("/get/get-albums")
        .cookie(cookie("own-root", true))
        .dispatch()
        .into_json()
        .expect("get-albums json");
    let entry = albums
        .as_array()
        .expect("album array")
        .iter()
        .find(|a| a["albumId"].as_str() == Some(album_id.as_str()))
        .expect("album listed")["shareList"]
        .get(&fp)
        .expect("share listed");
    assert_eq!(
        entry["keyFingerprint"].as_str(),
        Some(fp.as_str()),
        "entry must carry the fingerprint, got {entry}"
    );
    assert!(
        entry.get("url").is_none(),
        "ShareInfo must not carry url, got {entry}"
    );

    // Admin sees foreign shares too, by fingerprint only.
    let other_keys = listed_share_urls(&client, "own-other", true, &album_id);
    assert!(
        other_keys.contains(&fp),
        "admin must see foreign share by fingerprint, got {other_keys:?}"
    );
    assert!(
        !other_keys.contains(&share_id),
        "admin listing must not be keyed by the bearer, got {other_keys:?}"
    );

    // No bearer value anywhere in the admin's raw response.
    let raw = client
        .get("/get/get-albums")
        .cookie(cookie("own-other", true))
        .dispatch()
        .into_string()
        .expect("get-albums body");
    assert!(
        !raw.contains(&share_id),
        "admin listing must not contain the bearer anywhere, got: {raw}"
    );
}

/// A non-admin sees none of an admin's shares on the shared page input.
#[test]
fn non_admin_list_omits_foreign_shares() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/owned2/photo.jpg");
    seed("own-root2", true, None);
    seed("own-viewer", false, None);

    let share_id = create_share_as(&client, "own-root2", true, &album_id);

    let viewer_shares = listed_share_urls(&client, "own-viewer", false, &album_id);
    assert!(
        !viewer_shares.contains(&share_fingerprint_for(&share_id)),
        "viewer must not see admin shares, got {viewer_shares:?}"
    );
}

/// Editing a share as another user is refused: the share's creator is
/// unchanged and the editor gains no visibility.
#[test]
fn edit_by_other_user_refused_preserves_creator() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/owned3/photo.jpg");
    seed("own-root3", false, None);
    seed("own-other3", false, None);

    let share_id = create_share_as(&client, "own-root3", false, &album_id);

    let edit = client
        .put("/put/edit_share")
        .cookie(cookie("own-other3", false))
        .header(ContentType::JSON)
        .body(
            json!({
                "albumId": album_id,
                "share": {
                    "url": share_fingerprint_for(&share_id),
                    "description": "renamed by other admin",
                    "password": null,
                    "showMetadata": true,
                    "showDownload": true,
                    "showUpload": false,
                    "exp": 0
                }
            })
            .to_string(),
        )
        .dispatch();
    assert_eq!(edit.status(), Status::Forbidden, "edit_share as other user");

    let root_shares = listed_share_urls(&client, "own-root3", false, &album_id);
    assert!(
        root_shares.contains(&share_fingerprint_for(&share_id)),
        "original creator still sees the edited share"
    );
    let other_shares = listed_share_urls(&client, "own-other3", false, &album_id);
    assert!(
        !other_shares.contains(&share_fingerprint_for(&share_id)),
        "editor must not gain visibility, got {other_shares:?}"
    );
}

/// Deleting a share removes it from every listing.
#[test]
fn delete_share_removes_it_from_listings() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/ownedel/photo.jpg");
    seed("own-root4", true, None);

    let share_id = create_share_as(&client, "own-root4", true, &album_id);
    assert!(
        listed_share_urls(&client, "own-root4", true, &album_id)
            .contains(&share_fingerprint_for(&share_id)),
        "share listed before delete"
    );

    let delete = client
        .put("/put/delete_share")
        .cookie(cookie("own-root4", true))
        .header(ContentType::JSON)
        .body(
            json!({ "albumId": album_id, "shareId": share_fingerprint_for(&share_id) }).to_string(),
        )
        .dispatch();
    assert_eq!(delete.status(), Status::Ok, "delete_share");
    assert!(
        !listed_share_urls(&client, "own-root4", true, &album_id)
            .contains(&share_fingerprint_for(&share_id)),
        "share gone from listing after delete"
    );
}

// ── S4 personal-album share management ───────────────────────────────

pub fn enable_users_home() -> PathBuf {
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

pub fn personal_album_id(users_home: &std::path::Path, user: &str) -> String {
    let dir = users_home.join(user);
    crate::process::dir_album::get_album_id_for_dir(&dir)
        .expect("personal home album id")
        .to_string()
}

fn prefetch_as_share_anon(
    client: &rocket::local::blocking::Client,
    album_id: &str,
    share_id: &str,
) -> rocket::http::Status {
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
        .header(ContentType::JSON)
        .dispatch()
        .status()
}

#[test]
fn s4_non_admin_personal_share_crud_and_resolve() {
    let _guard = lock();
    let client = setup_image_home();
    let users_home = enable_users_home();
    seed("pers-user", false, None);
    let album_id = personal_album_id(&users_home, "pers-user");

    let share_id = create_share_as(&client, "pers-user", false, &album_id);

    // edit
    let edit = client
        .put("/put/edit_share")
        .cookie(cookie("pers-user", false))
        .header(ContentType::JSON)
        .body(
            json!({
                "albumId": album_id,
                "share": {
                    "url": share_fingerprint_for(&share_id),
                    "description": "edited",
                    "password": null,
                    "showMetadata": true,
                    "showDownload": true,
                    "showUpload": false,
                    "exp": 0
                }
            })
            .to_string(),
        )
        .dispatch();
    assert_eq!(edit.status(), Status::Ok, "edit own personal share");

    // resolve round-trip for anonymous holder
    let status = prefetch_as_share_anon(&client, &album_id, &share_id);
    assert_eq!(status, Status::Ok, "personal share resolves anonymously");

    // delete
    let delete = client
        .put("/put/delete_share")
        .cookie(cookie("pers-user", false))
        .header(ContentType::JSON)
        .body(
            json!({ "albumId": album_id, "shareId": share_fingerprint_for(&share_id) }).to_string(),
        )
        .dispatch();
    assert_eq!(delete.status(), Status::Ok, "delete own personal share");
}

#[test]
fn s4_non_admin_creates_share_on_public_album() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/s4pub/photo.jpg");
    seed("pub-user", false, None);
    let share_id = create_share_as(&client, "pub-user", false, &album_id);
    assert_ne!(share_id, "");
}

#[test]
fn s4_non_admin_refused_on_others_personal_album() {
    let _guard = lock();
    let client = setup_image_home();
    let users_home = enable_users_home();
    seed("pers-a", false, None);
    seed("pers-b", false, None);
    let album_id = personal_album_id(&users_home, "pers-a");

    let create = client
        .post("/post/create_share")
        .cookie(cookie("pers-b", false))
        .header(ContentType::JSON)
        .body(share_body(&album_id).to_string())
        .dispatch();
    assert_eq!(create.status(), Status::NotFound, "other's personal album");

    // target unchanged: no shares on the album for its owner
    let urls = listed_share_urls(&client, "pers-a", false, &album_id);
    assert!(urls.is_empty(), "no share created, got {urls:?}");
}

#[test]
fn s4_admin_refused_on_others_personal_album() {
    let _guard = lock();
    let client = setup_image_home();
    let users_home = enable_users_home();
    seed("pers-a2", false, None);
    seed("admin", true, None);
    let album_id = personal_album_id(&users_home, "pers-a2");

    let create = client
        .post("/post/create_share")
        .cookie(cookie("admin", true))
        .header(ContentType::JSON)
        .body(share_body(&album_id).to_string())
        .dispatch();
    assert_eq!(
        create.status(),
        Status::NotFound,
        "admin on other's personal"
    );
}

#[test]
fn s4_user_b_cannot_edit_or_delete_users_share_on_public_album() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/s4shared/photo.jpg");
    seed("user-a", false, None);
    seed("user-b", false, None);

    let share_id = create_share_as(&client, "user-a", false, &album_id);
    assert!(
        listed_share_urls(&client, "user-a", false, &album_id)
            .contains(&share_fingerprint_for(&share_id))
    );

    // edit
    let edit = client
        .put("/put/edit_share")
        .cookie(cookie("user-b", false))
        .header(ContentType::JSON)
        .body(
            json!({
                "albumId": album_id,
                "share": {
                    "url": share_fingerprint_for(&share_id),
                    "description": "hijacked",
                    "password": null,
                    "showMetadata": true,
                    "showDownload": true,
                    "showUpload": false,
                    "exp": 0
                }
            })
            .to_string(),
        )
        .dispatch();
    assert_eq!(edit.status(), Status::Forbidden, "edit by other user");

    // delete
    let delete = client
        .put("/put/delete_share")
        .cookie(cookie("user-b", false))
        .header(ContentType::JSON)
        .body(
            json!({ "albumId": album_id, "shareId": share_fingerprint_for(&share_id) }).to_string(),
        )
        .dispatch();
    assert_eq!(delete.status(), Status::Forbidden, "delete by other user");

    // share unchanged
    assert!(
        listed_share_urls(&client, "user-a", false, &album_id)
            .contains(&share_fingerprint_for(&share_id)),
        "share must remain after refused edit/delete"
    );
}

#[test]
fn s4_personal_share_resolves_anonymously_with_flags() {
    let _guard = lock();
    let client = setup_image_home();
    let users_home = enable_users_home();
    seed("flag-user", false, None);
    let album_id = personal_album_id(&users_home, "flag-user");

    // create share with show_metadata false
    let create = client
        .post("/post/create_share")
        .cookie(cookie("flag-user", false))
        .header(ContentType::JSON)
        .body(
            json!({
                "albumId": album_id,
                "description": "flagged",
                "password": null,
                "showMetadata": false,
                "showDownload": true,
                "showUpload": false,
                "exp": 0
            })
            .to_string(),
        )
        .dispatch();
    assert_eq!(create.status(), Status::Ok, "create personal share");
    let share_id = create.into_string().expect("share id").trim().to_string();

    let status = prefetch_as_share_anon(&client, &album_id, &share_id);
    assert_eq!(status, Status::Ok, "personal share resolves anonymously");
}

#[test]
fn s4_disabled_mode_non_admin_can_create_public_share() {
    let _guard = lock();
    let client = setup_image_home(); // users_home stays None
    let (album_id, _asset_id) = create_album(&client, "/s4disabled/photo.jpg");
    seed("disabled-user", false, None);
    let share_id = create_share_as(&client, "disabled-user", false, &album_id);
    assert_ne!(share_id, "");
}

#[test]
fn move_album_with_foreign_share_refused_and_tree_unchanged() {
    let _guard = lock();
    let client = setup_image_home();
    let users_home = enable_users_home();
    seed("bob", false, None);
    seed("alice", false, None);

    let (album_id, _asset_id) = create_album(&client, "/shared/pub/photo.jpg");
    let share_id = create_share_as(&client, "bob", false, &album_id);

    let alice_personal = personal_album_id(&users_home, "alice");

    let resp = client
        .put("/put/assign_album")
        .cookie(cookie("alice", false))
        .header(ContentType::JSON)
        .body(
            json!({
                "assetId": album_id,
                "albumId": alice_personal,
                "onConflict": "rename"
            })
            .to_string(),
        )
        .dispatch();
    assert_eq!(resp.status(), Status::Forbidden);

    let bob_shares = listed_share_urls(&client, "bob", false, &album_id);
    assert!(bob_shares.contains(&share_fingerprint_for(&share_id)));
    let albums: Value = client
        .get("/get/get-albums")
        .cookie(cookie("bob", false))
        .dispatch()
        .into_json()
        .expect("get-albums json");
    let albums_arr = albums.as_array().expect("album array");
    assert!(
        albums_arr
            .iter()
            .any(|a| a["albumId"].as_str() == Some(&album_id))
    );
}

#[test]
fn move_album_with_own_shares_succeeds() {
    let _guard = lock();
    let client = setup_image_home();
    let users_home = enable_users_home();
    seed("alice", false, None);

    let (album_id, _asset_id) = create_album(&client, "/owned/album/photo.jpg");
    let _share_id = create_share_as(&client, "alice", false, &album_id);
    let alice_personal = personal_album_id(&users_home, "alice");

    let resp = client
        .put("/put/assign_album")
        .cookie(cookie("alice", false))
        .header(ContentType::JSON)
        .body(
            json!({
                "assetId": album_id,
                "albumId": alice_personal,
                "onConflict": "rename"
            })
            .to_string(),
        )
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
}

#[test]
fn admin_moving_album_with_another_users_share_refused() {
    let _guard = lock();
    let client = setup_image_home();
    let users_home = enable_users_home();
    seed("bob", false, None);
    seed("admin", true, None);

    let (album_id, _asset_id) = create_album(&client, "/shared/pub2/photo.jpg");
    let _share_id = create_share_as(&client, "bob", false, &album_id);
    let admin_personal = personal_album_id(&users_home, "admin");

    let resp = client
        .put("/put/assign_album")
        .cookie(cookie("admin", true))
        .header(ContentType::JSON)
        .body(
            json!({
                "assetId": album_id,
                "albumId": admin_personal,
                "onConflict": "rename"
            })
            .to_string(),
        )
        .dispatch();
    assert_eq!(resp.status(), Status::Forbidden);
}

// ── S2: creator-only share-token reveal ─────────────────────────────

/// Non-secret handle for a share bearer: mirrors
/// `Share::fingerprint` without reaching into the stored record.
fn share_fingerprint_for(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()[..16].to_string()
}

fn reveal_status(
    client: &rocket::local::blocking::Client,
    user: &str,
    admin: bool,
    album_id: &str,
    fingerprint: &str,
) -> rocket::http::Status {
    client
        .get(format!(
            "/get/share/token?album_id={album_id}&fingerprint={fingerprint}"
        ))
        .cookie(cookie(user, admin))
        .dispatch()
        .status()
}

#[test]
fn s2_creator_reveals_own_share_bearer() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/s2reveal/photo.jpg");
    seed("rev-a", false, None);
    let share_id = create_share_as(&client, "rev-a", false, &album_id);
    let fp = share_fingerprint_for(&share_id);

    let resp = client
        .get(format!(
            "/get/share/token?album_id={album_id}&fingerprint={fp}"
        ))
        .cookie(cookie("rev-a", false))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "creator reveal");
    let body: Value = resp.into_json().expect("reveal json");
    assert_eq!(
        body["token"].as_str(),
        Some(share_id.as_str()),
        "revealed bearer must match"
    );

    // The revealed bearer actually works anonymously.
    assert_eq!(
        prefetch_as_share_anon(&client, &album_id, &share_id),
        Status::Ok,
        "revealed bearer resolves"
    );
}

#[test]
fn s2_other_user_cannot_reveal_foreign_share() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/s2foreign/photo.jpg");
    seed("rev-owner", false, None);
    seed("rev-bob", false, None);
    let share_id = create_share_as(&client, "rev-owner", false, &album_id);
    let fp = share_fingerprint_for(&share_id);

    let resp = client
        .get(format!(
            "/get/share/token?album_id={album_id}&fingerprint={fp}"
        ))
        .cookie(cookie("rev-bob", false))
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::NotFound,
        "bob must not reveal alice's share"
    );
    let body = resp.into_string().expect("error body");
    assert!(
        !body.contains(&share_id),
        "refusal must not leak the bearer"
    );
}

#[test]
fn s2_admin_cannot_reveal_foreign_share() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/s2admin/photo.jpg");
    seed("rev-owner2", false, None);
    seed("rev-admin", true, None);
    let share_id = create_share_as(&client, "rev-owner2", false, &album_id);
    let fp = share_fingerprint_for(&share_id);

    assert_eq!(
        reveal_status(&client, "rev-admin", true, &album_id, &fp),
        Status::NotFound,
        "admin must not reveal another user's share"
    );
}

#[test]
fn s2_unknown_fingerprint_is_not_found() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/s2unknown/photo.jpg");
    seed("rev-owner3", false, None);
    create_share_as(&client, "rev-owner3", false, &album_id);

    assert_eq!(
        reveal_status(&client, "rev-owner3", false, &album_id, "0123456789abcdef"),
        Status::NotFound,
        "unknown fingerprint"
    );
}

#[test]
fn s2_reveal_on_invisible_personal_album_is_not_found() {
    let _guard = lock();
    let client = setup_image_home();
    let users_home = enable_users_home();
    seed("rev-pa", false, None);
    seed("rev-pb", false, None);
    let album_id = personal_album_id(&users_home, "rev-pa");
    let share_id = create_share_as(&client, "rev-pa", false, &album_id);
    let fp = share_fingerprint_for(&share_id);

    assert_eq!(
        reveal_status(&client, "rev-pb", false, &album_id, &fp),
        Status::NotFound,
        "bob must not reveal through alice's personal album"
    );
}

// ── S3: edit and revoke by fingerprint ──────────────────────────────

#[test]
fn s3_edit_by_fingerprint_as_creator_ok() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/s3edit/photo.jpg");
    seed("s3creator", false, None);
    // Use admin-created album? create_album uses admin cookie. Share created by s3creator? Need album visible to s3creator.
    // Simpler: create share as admin (creator admin) on admin-visible album.
    // Reuse admin as creator.
    let share_id = create_share_as(&client, "admin", true, &album_id);
    let fp = share_fingerprint_for(&share_id);

    let edit = client
        .put("/put/edit_share")
        .cookie(cookie("admin", true))
        .header(ContentType::JSON)
        .body(
            json!({
                "albumId": album_id,
                "share": {
                    "url": fp,
                    "description": "renamed",
                    "password": null,
                    "showMetadata": true,
                    "showDownload": true,
                    "showUpload": false,
                    "exp": 0
                }
            })
            .to_string(),
        )
        .dispatch();
    assert_eq!(edit.status(), Status::Ok, "edit by creator");

    // fingerprint remains the listing key after edit, creator unchanged
    let urls = listed_share_urls(&client, "admin", true, &album_id);
    assert!(
        urls.contains(&share_fingerprint_for(&share_id)),
        "fingerprint must remain the listing key after edit, got {urls:?}"
    );
}

#[test]
fn s3_edit_by_other_user_refused() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/s3editother/photo.jpg");
    seed("s3owner", false, None);
    seed("s3other", false, None);
    let share_id = create_share_as(&client, "s3owner", false, &album_id);
    let fp = share_fingerprint_for(&share_id);

    let edit = client
        .put("/put/edit_share")
        .cookie(cookie("s3other", false))
        .header(ContentType::JSON)
        .body(
            json!({
                "albumId": album_id,
                "share": {
                    "url": fp,
                    "description": "hijacked",
                    "password": null,
                    "showMetadata": true,
                    "showDownload": true,
                    "showUpload": false,
                    "exp": 0
                }
            })
            .to_string(),
        )
        .dispatch();
    assert_eq!(edit.status(), Status::Forbidden, "edit by other user");
}

#[test]
fn s3_edit_by_admin_non_creator_refused() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/s3editadmin/photo.jpg");
    seed("s3owner2", false, None);
    seed("s3admin", true, None);
    let share_id = create_share_as(&client, "s3owner2", false, &album_id);
    let fp = share_fingerprint_for(&share_id);

    let edit = client
        .put("/put/edit_share")
        .cookie(cookie("s3admin", true))
        .header(ContentType::JSON)
        .body(
            json!({
                "albumId": album_id,
                "share": {
                    "url": fp,
                    "description": "hijacked by admin",
                    "password": null,
                    "showMetadata": true,
                    "showDownload": true,
                    "showUpload": false,
                    "exp": 0
                }
            })
            .to_string(),
        )
        .dispatch();
    assert_eq!(
        edit.status(),
        Status::Forbidden,
        "admin non-creator edit refused"
    );
}

#[test]
fn s3_delete_by_fingerprint_as_creator_ok() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/s3del/photo.jpg");
    seed("s3delowner", false, None);
    let share_id = create_share_as(&client, "s3delowner", false, &album_id);
    let fp = share_fingerprint_for(&share_id);
    assert!(
        listed_share_urls(&client, "s3delowner", false, &album_id)
            .contains(&share_fingerprint_for(&share_id)),
        "share listed before delete"
    );

    let delete = client
        .put("/put/delete_share")
        .cookie(cookie("s3delowner", false))
        .header(ContentType::JSON)
        .body(json!({ "albumId": album_id, "shareId": fp }).to_string())
        .dispatch();
    assert_eq!(delete.status(), Status::Ok, "delete by creator");
    assert!(
        !listed_share_urls(&client, "s3delowner", false, &album_id)
            .contains(&share_fingerprint_for(&share_id)),
        "share gone after delete"
    );
}

#[test]
fn s3_delete_by_admin_of_foreign_share_ok() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/s3deladmin/photo.jpg");
    seed("s3foreignowner", false, None);
    seed("s3admin2", true, None);
    let share_id = create_share_as(&client, "s3foreignowner", false, &album_id);
    let fp = share_fingerprint_for(&share_id);

    let delete = client
        .put("/put/delete_share")
        .cookie(cookie("s3admin2", true))
        .header(ContentType::JSON)
        .body(json!({ "albumId": album_id, "shareId": fp }).to_string())
        .dispatch();
    assert_eq!(delete.status(), Status::Ok, "admin revoke of foreign share");
    // verify gone from owner listing
    assert!(
        !listed_share_urls(&client, "s3foreignowner", false, &album_id)
            .contains(&share_fingerprint_for(&share_id)),
        "share gone after admin revoke"
    );
}

#[test]
fn s3_delete_by_non_admin_non_creator_refused() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/s3delrefused/photo.jpg");
    seed("s3owner3", false, None);
    seed("s3other3", false, None);
    let share_id = create_share_as(&client, "s3owner3", false, &album_id);
    let fp = share_fingerprint_for(&share_id);

    let delete = client
        .put("/put/delete_share")
        .cookie(cookie("s3other3", false))
        .header(ContentType::JSON)
        .body(json!({ "albumId": album_id, "shareId": fp }).to_string())
        .dispatch();
    assert_eq!(
        delete.status(),
        Status::Forbidden,
        "delete by non-creator non-admin"
    );
}

#[test]
fn s3_delete_unknown_fingerprint_is_noop() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/s3delunknown/photo.jpg");
    seed("s3owner4", false, None);
    let _share_id = create_share_as(&client, "s3owner4", false, &album_id);

    let delete = client
        .put("/put/delete_share")
        .cookie(cookie("s3owner4", false))
        .header(ContentType::JSON)
        .body(json!({ "albumId": album_id, "shareId": "0123456789abcdef" }).to_string())
        .dispatch();
    assert_eq!(delete.status(), Status::Ok, "unknown fingerprint no-op");
}
