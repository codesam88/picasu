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

/// Admins see their own shares; another admin does not.
#[test]
fn listings_scope_shares_to_their_creator() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/owned/photo.jpg");
    seed("own-root", true, None);
    seed("own-other", true, None);

    let share_id = create_share_as(&client, "own-root", true, &album_id);

    let root_shares = listed_share_urls(&client, "own-root", true, &album_id);
    assert!(
        root_shares.contains(&share_id),
        "creator still sees own share"
    );
    let other_shares = listed_share_urls(&client, "own-other", true, &album_id);
    assert!(
        !other_shares.contains(&share_id),
        "other admin must not see it, got {other_shares:?}"
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
        !viewer_shares.contains(&share_id),
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
    seed("own-root3", true, None);
    seed("own-other3", true, None);

    let share_id = create_share_as(&client, "own-root3", true, &album_id);

    let edit = client
        .put("/put/edit_share")
        .cookie(cookie("own-other3", true))
        .header(ContentType::JSON)
        .body(
            json!({
                "albumId": album_id,
                "share": {
                    "url": share_id,
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

    let root_shares = listed_share_urls(&client, "own-root3", true, &album_id);
    assert!(
        root_shares.contains(&share_id),
        "original creator still sees the edited share"
    );
    let other_shares = listed_share_urls(&client, "own-other3", true, &album_id);
    assert!(
        !other_shares.contains(&share_id),
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
        listed_share_urls(&client, "own-root4", true, &album_id).contains(&share_id),
        "share listed before delete"
    );

    let delete = client
        .put("/put/delete_share")
        .cookie(cookie("own-root4", true))
        .header(ContentType::JSON)
        .body(json!({ "albumId": album_id, "shareId": share_id }).to_string())
        .dispatch();
    assert_eq!(delete.status(), Status::Ok, "delete_share");
    assert!(
        !listed_share_urls(&client, "own-root4", true, &album_id).contains(&share_id),
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
                    "url": share_id,
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
        .body(json!({ "albumId": album_id, "shareId": share_id }).to_string())
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
    assert!(listed_share_urls(&client, "user-a", false, &album_id).contains(&share_id));

    // edit
    let edit = client
        .put("/put/edit_share")
        .cookie(cookie("user-b", false))
        .header(ContentType::JSON)
        .body(
            json!({
                "albumId": album_id,
                "share": {
                    "url": share_id,
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
        .body(json!({ "albumId": album_id, "shareId": share_id }).to_string())
        .dispatch();
    assert_eq!(delete.status(), Status::Forbidden, "delete by other user");

    // share unchanged
    assert!(
        listed_share_urls(&client, "user-a", false, &album_id).contains(&share_id),
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
    assert!(bob_shares.contains(&share_id));
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
