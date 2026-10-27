#![cfg(test)]

//! Share passwords leave the backend only as presence (`hasPassword`) in
//! album reads. `edit_share` keeps the stored secret when the submitted
//! `password` is absent or `null`, sets it on a non-empty string, and clears
//! it on the empty string. The share capability chain is untouched: a bearer
//! who presented the password still receives it back in the resolved share,
//! because media requests resend it as `x-share-password`.

use rocket::http::{ContentType, Header, Status};
use serde_json::{Value, json};

use crate::model::metadata_record::MetadataRecord;
use crate::storage::db::{METADATA_TABLE, TREE};
use crate::tests::bootstrap::{cookie, lock, seed, setup_image_home};
use crate::tests::fixtures::authz::create_album;
use crate::tests::fixtures::test_password;
use redb::{ReadableDatabase, ReadableTable};

/// `POST /get/prefetch` as the album's share, with an optional password.
fn share_prefetch(
    client: &rocket::local::blocking::Client,
    album_id: &str,
    share_id: &str,
    password: Option<&str>,
) -> Status {
    let mut req = client
        .post("/get/prefetch")
        .header(Header::new("x-album-id", album_id.to_string()))
        .header(Header::new("x-share-id", share_id.to_string()))
        .header(ContentType::JSON);
    if let Some(password) = password {
        req = req.header(Header::new("x-share-password", password.to_string()));
    }
    req.dispatch().status()
}

fn stored_share_password(album_id: &str, share_id: &str) -> Option<String> {
    let read_txn = TREE.in_disk.begin_read().expect("begin read");
    let table = read_txn.open_table(METADATA_TABLE).expect("open table");
    let guard = table
        .get(album_id)
        .expect("get album")
        .expect("album exists");
    let MetadataRecord::Album(album) = guard.value() else {
        panic!("not an album");
    };
    album
        .share_list
        .iter()
        .find(|(k, _)| k.as_str() == share_id)
        .and_then(|(_, share)| share.password.clone())
}

fn set_legacy_share_password(album_id: &str, share_id: &str, password: &str) {
    let txn = TREE.in_disk.begin_write().expect("begin write");
    {
        let mut table = txn.open_table(METADATA_TABLE).expect("open table");
        let mut album = {
            let guard = table
                .get(album_id)
                .expect("get album")
                .expect("album exists");
            match guard.value() {
                MetadataRecord::Album(a) => a,
                _ => panic!("not an album"),
            }
        };
        for (key, share) in &mut album.share_list {
            if key.as_str() == share_id {
                share.password = Some(password.to_string());
            }
        }
        table
            .insert(album_id, MetadataRecord::Album(album))
            .expect("insert album");
    }
    txn.commit().expect("commit");
}

/// Album reads carry share presence, never the secret — for the admin and
/// for a viewer alike — while the share itself still authenticates with the
/// stored password.
#[test]
fn album_reads_carry_share_presence_not_password() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/sharepw/photo.jpg");
    seed(
        "pw-reader",
        false,
        Some(test_password("t-reader-pw").as_str()),
    );

    let create = client
        .post("/post/create_share")
        .cookie(cookie("admin", true))
        .header(ContentType::JSON)
        .body(
            json!({
                "albumId": album_id,
                "description": "guarded",
                "password": "s3cret-share-pw",
                "showMetadata": true,
                "showDownload": true,
                "showUpload": false,
                "exp": 0
            })
            .to_string(),
        )
        .dispatch();
    assert_eq!(create.status(), Status::Ok, "create_share");
    let share_id = create.into_string().expect("share id").trim().to_string();

    for (label, request_cookie) in [
        ("admin", cookie("admin", true)),
        ("viewer", cookie("pw-reader", false)),
    ] {
        let raw = client
            .get("/get/get-albums")
            .cookie(request_cookie)
            .dispatch()
            .into_string()
            .expect("get-albums body");
        assert!(
            !raw.contains("\"password\""),
            "{label} album read must not name a password field, got: {raw}"
        );
        let albums: Value = serde_json::from_str(&raw).expect("get-albums json");
        let entry = albums
            .as_array()
            .expect("album array")
            .iter()
            .find(|a| a["albumId"].as_str() == Some(album_id.as_str()))
            .expect("album listed")["shareList"]
            .get(&share_id)
            .expect("share listed");
        assert_eq!(
            entry["hasPassword"], true,
            "{label} read must surface presence, got: {entry}"
        );
    }

    // The capability chain is unaffected: the stored password still works,
    // a wrong one does not, and no password still does.
    assert_eq!(
        share_prefetch(&client, &album_id, &share_id, Some("s3cret-share-pw")),
        Status::Ok,
        "correct share password"
    );
    assert_ne!(
        share_prefetch(&client, &album_id, &share_id, Some("wrong")),
        Status::Ok,
        "wrong share password"
    );
}

/// The stored share entry as the album listing reports it (presence only).
fn listed_share(client: &rocket::local::blocking::Client, album_id: &str, share_id: &str) -> Value {
    let albums: Value = client
        .get("/get/get-albums")
        .cookie(cookie("admin", true))
        .dispatch()
        .into_json()
        .expect("get-albums json");
    albums
        .as_array()
        .expect("album array")
        .iter()
        .find(|a| a["albumId"].as_str() == Some(album_id))
        .expect("album listed")["shareList"]
        .get(share_id)
        .expect("share listed")
        .clone()
}

/// `edit_share` password contract: absent or `null` keeps the stored secret,
/// a non-empty string replaces it, the empty string clears it.
#[test]
fn edit_share_keeps_sets_and_clears_password() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/shareedit/photo.jpg");

    let create = client
        .post("/post/create_share")
        .cookie(cookie("admin", true))
        .header(ContentType::JSON)
        .body(
            json!({
                "albumId": album_id,
                "description": "edit me",
                "password": "first-pw",
                "showMetadata": true,
                "showDownload": true,
                "showUpload": false,
                "exp": 0
            })
            .to_string(),
        )
        .dispatch();
    assert_eq!(create.status(), Status::Ok, "create_share");
    let share_id = create.into_string().expect("share id").trim().to_string();
    // Recover the stored url: only `url` keys the replace, so read it once
    // through the album listing (the password itself is not exposed).
    let stored = listed_share(&client, &album_id, &share_id);

    let edit = |password: Value| {
        client
            .put("/put/edit_share")
            .cookie(cookie("admin", true))
            .header(ContentType::JSON)
            .body(
                json!({
                    "albumId": album_id,
                    "share": {
                        "url": stored["url"],
                        "description": "edit me",
                        "password": password,
                        "showMetadata": true,
                        "showDownload": true,
                        "showUpload": false,
                        "exp": 0
                    }
                })
                .to_string(),
            )
            .dispatch()
            .status()
    };

    // null keeps: the first password still authenticates.
    assert_eq!(edit(Value::Null), Status::Ok, "edit with null password");
    assert_eq!(
        share_prefetch(&client, &album_id, &share_id, Some("first-pw")),
        Status::Ok,
        "null must keep the stored password"
    );

    // A string sets: the old secret dies, the new one works.
    assert_eq!(
        edit(Value::String("second-pw".to_string())),
        Status::Ok,
        "edit with new password"
    );
    assert_eq!(
        share_prefetch(&client, &album_id, &share_id, Some("second-pw")),
        Status::Ok,
        "new password must work"
    );
    assert_ne!(
        share_prefetch(&client, &album_id, &share_id, Some("first-pw")),
        Status::Ok,
        "old password must be dead"
    );

    // The empty string clears.
    assert_eq!(
        edit(Value::String(String::new())),
        Status::Ok,
        "clear password"
    );
    let entry = listed_share(&client, &album_id, &share_id);
    assert_eq!(
        entry["hasPassword"], false,
        "cleared share must report no password, got: {entry}"
    );
    // A share without a password gates nothing: any bearer of the link
    // passes, with or without a password header.
    assert_eq!(
        share_prefetch(&client, &album_id, &share_id, None),
        Status::Ok,
        "cleared share must accept a passwordless request"
    );
}

#[test]
fn share_password_is_hashed_at_rest() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/sharehash/photo.jpg");

    let create = client
        .post("/post/create_share")
        .cookie(cookie("admin", true))
        .header(ContentType::JSON)
        .body(
            json!({
                "albumId": album_id,
                "description": "hashed",
                "password": "s3cret-share-pw",
                "showMetadata": true,
                "showDownload": true,
                "showUpload": false,
                "exp": 0
            })
            .to_string(),
        )
        .dispatch();
    assert_eq!(create.status(), Status::Ok, "create_share");
    let share_id = create.into_string().expect("share id").trim().to_string();

    let stored = stored_share_password(&album_id, &share_id).expect("stored password");
    assert!(
        stored.starts_with("$argon2id$"),
        "stored share password must be argon2id, got: {stored}"
    );
    assert!(
        !stored.contains("s3cret-share-pw"),
        "stored value must not contain the plaintext password"
    );
}

#[test]
fn a_hashed_share_password_grants_access() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/shareaccess/photo.jpg");

    let create = client
        .post("/post/create_share")
        .cookie(cookie("admin", true))
        .header(ContentType::JSON)
        .body(
            json!({
                "albumId": album_id,
                "description": "guarded",
                "password": "s3cret-share-pw",
                "showMetadata": true,
                "showDownload": true,
                "showUpload": false,
                "exp": 0
            })
            .to_string(),
        )
        .dispatch();
    assert_eq!(create.status(), Status::Ok, "create_share");
    let share_id = create.into_string().expect("share id").trim().to_string();

    assert_eq!(
        share_prefetch(&client, &album_id, &share_id, Some("s3cret-share-pw")),
        Status::Ok,
        "correct share password"
    );
    assert_eq!(
        share_prefetch(&client, &album_id, &share_id, Some("wrong")),
        Status::Unauthorized,
        "wrong share password"
    );
}

#[test]
fn a_legacy_plaintext_share_row_still_grants_access() {
    let _guard = lock();
    let client = setup_image_home();
    let (album_id, _asset_id) = create_album(&client, "/sharelegacy/photo.jpg");

    let create = client
        .post("/post/create_share")
        .cookie(cookie("admin", true))
        .header(ContentType::JSON)
        .body(
            json!({
                "albumId": album_id,
                "description": "legacy",
                "password": null,
                "showMetadata": true,
                "showDownload": true,
                "showUpload": false,
                "exp": 0
            })
            .to_string(),
        )
        .dispatch();
    assert_eq!(create.status(), Status::Ok, "create_share");
    let share_id = create.into_string().expect("share id").trim().to_string();

    // Simulate a pre-hashing plaintext row (no migration).
    set_legacy_share_password(&album_id, &share_id, "legacy-pw");

    assert_eq!(
        share_prefetch(&client, &album_id, &share_id, Some("legacy-pw")),
        Status::Ok,
        "legacy plaintext password"
    );

    let stored = stored_share_password(&album_id, &share_id).expect("stored password");
    assert_eq!(stored, "legacy-pw", "legacy row must not be rewritten");
}
