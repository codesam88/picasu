//! Per-user share ownership: album listings show only the shares the
//! caller created. The creator id is stamped server-side on create and
//! preserved across edits, so the filter reads it straight off the row.

#![cfg(test)]

use rocket::http::{ContentType, Status};
use serde_json::{Value, json};

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

/// Editing a share as another admin preserves the original creator: the
/// owner still lists it afterwards.
#[test]
fn edit_by_other_admin_preserves_creator() {
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
    assert_eq!(edit.status(), Status::Ok, "edit_share as other admin");

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
