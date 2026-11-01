#![cfg(test)]

//! Personal-area read isolation: a user sees their own personal album and
//! never another user's, and the `users_home` root itself is never listed.

use rocket::http::{ContentType, Status};
use serde_json::{Value, json};

use crate::tests::bootstrap::{cookie, lock, seed, setup_image_home};
use crate::tests::share_ownership::{enable_users_home, personal_album_id};

#[test]
fn get_albums_omits_other_users_personal_album() {
    let _guard = lock();
    let client = setup_image_home();
    let users_home = enable_users_home();
    seed("read-a", false, None);
    seed("read-b", false, None);

    let a_album = personal_album_id(&users_home, "read-a");
    let b_album = personal_album_id(&users_home, "read-b");

    // As read-a: own album present, read-b's absent.
    let albums: Value = client
        .get("/get/get-albums")
        .cookie(cookie("read-a", false))
        .dispatch()
        .into_json()
        .expect("get-albums json");
    let ids: Vec<&str> = albums
        .as_array()
        .expect("album array")
        .iter()
        .filter_map(|a| a["albumId"].as_str())
        .collect();
    assert!(ids.contains(&a_album.as_str()), "own album missing");
    assert!(
        !ids.contains(&b_album.as_str()),
        "other user's personal album leaked: {ids:?}"
    );

    // root/owner markers on the own album
    let own = albums
        .as_array()
        .expect("album array")
        .iter()
        .find(|a| a["albumId"].as_str() == Some(&a_album))
        .expect("own album row");
    assert_eq!(own["root"], "personal");
    assert_eq!(own["owner"], "read-a");

    // As read-b: the mirror image.
    let albums: Value = client
        .get("/get/get-albums")
        .cookie(cookie("read-b", false))
        .dispatch()
        .into_json()
        .expect("get-albums json");
    let ids: Vec<&str> = albums
        .as_array()
        .expect("album array")
        .iter()
        .filter_map(|a| a["albumId"].as_str())
        .collect();
    assert!(ids.contains(&b_album.as_str()), "own album missing");
    assert!(
        !ids.contains(&a_album.as_str()),
        "other user's album leaked"
    );

    // users_home root itself never appears (no album with root == null personal root)
    assert!(
        !albums
            .as_array()
            .expect("album array")
            .iter()
            .any(|a| a["root"].is_null() && a["dirPath"].as_str() == Some("")),
        "users root leaked"
    );
}

fn wait_for_index_completion() {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let status = crate::tasks::actor::album_index::album_index_status();
        match status.state {
            crate::tasks::actor::album_index::AlbumIndexState::Running => {}
            _ => return,
        }
        assert!(
            std::time::Instant::now() <= deadline,
            "index did not complete within 30s"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[test]
fn prefetch_include_personal_toggle_is_server_enforced() {
    let _guard = lock();
    let client = setup_image_home();
    let users_home = enable_users_home();
    seed("read-a", false, None);
    seed("read-b", false, None);

    let a_dir = users_home.join("read-a");
    let b_dir = users_home.join("read-b");
    std::fs::create_dir_all(&a_dir).expect("a dir");
    std::fs::create_dir_all(&b_dir).expect("b dir");
    let a_photo = a_dir.join("photo.jpg");
    let b_photo = b_dir.join("photo.jpg");
    snapfab::generate_batch(&[snapfab::PhotoSpec {
        output: Some(a_photo.to_string_lossy().into()),
        format: Some("jpeg".into()),
        width: Some(8),
        height: Some(8),
        tags: None,
        exif_date: None,
        minimal: false,
    }])
    .expect("gen a");
    snapfab::generate_batch(&[snapfab::PhotoSpec {
        output: Some(b_photo.to_string_lossy().into()),
        format: Some("jpeg".into()),
        width: Some(8),
        height: Some(8),
        tags: None,
        exif_date: None,
        minimal: false,
    }])
    .expect("gen b");

    crate::process::dir_album::reset_dir_album_cache();
    crate::tasks::actor::album_index::index_album_with_root("/", false, None)
        .expect("index accepted");
    wait_for_index_completion();

    let filter = serde_json::json!({
        "And": [
            {"Not": {"ExtType": "album"}},
            {"Trashed": false}
        ]
    });

    for user in ["read-a", "read-b"] {
        let resp = client
            .post("/get/prefetch")
            .cookie(cookie(user, false))
            .header(rocket::http::ContentType::JSON)
            .body(filter.to_string())
            .dispatch();
        assert_eq!(resp.status(), rocket::http::Status::Ok, "prefetch {user}");
        let body: serde_json::Value = resp.into_json().expect("prefetch json");
        assert_eq!(
            body["prefetch"]["dataLength"].as_u64(),
            Some(0),
            "{user} default must exclude personal photos"
        );
    }

    let resp = client
        .post("/get/prefetch?include_personal=true")
        .cookie(cookie("read-a", false))
        .header(rocket::http::ContentType::JSON)
        .body(filter.to_string())
        .dispatch();
    assert_eq!(
        resp.status(),
        rocket::http::Status::Ok,
        "prefetch read-a opt-in"
    );
    let body: serde_json::Value = resp.into_json().expect("prefetch json");
    assert_eq!(
        body["prefetch"]["dataLength"].as_u64(),
        Some(1),
        "read-a opt-in must include own photo"
    );

    let resp = client
        .post("/get/prefetch?include_personal=true")
        .cookie(cookie("read-b", false))
        .header(rocket::http::ContentType::JSON)
        .body(filter.to_string())
        .dispatch();
    assert_eq!(
        resp.status(),
        rocket::http::Status::Ok,
        "prefetch read-b opt-in"
    );
    let body: serde_json::Value = resp.into_json().expect("prefetch json");
    assert_eq!(
        body["prefetch"]["dataLength"].as_u64(),
        Some(1),
        "read-b opt-in must include own photo"
    );

    {
        let mut c = crate::model::config::APP_CONFIG
            .get()
            .expect("cfg")
            .write()
            .expect("lock");
        c.users_home = None;
    }
}

#[test]
fn prefetch_root_albums_respect_include_personal_flag() {
    let _guard = lock();
    let client = setup_image_home();
    let _users_home = enable_users_home();
    seed("read-a", false, None);
    seed("read-b", false, None);

    let image_home = crate::tests::bootstrap::test_image_home();
    let global_album = image_home.join("globalroot");
    std::fs::create_dir_all(&global_album).expect("global album dir");

    // index both roots (cache already holds the personal-home albums created
    // by `seed`, so the walk reuses them instead of duplicating)
    crate::tasks::actor::album_index::index_album_with_root("/", false, None)
        .expect("index accepted");
    wait_for_index_completion();

    let filter = serde_json::json!({
        "And": [
            {"ExtType": "album"},
            {"Trashed": false},
            {"RootAlbum": true}
        ]
    });

    // default (no flag): only global root album
    for user in ["read-a", "read-b"] {
        let resp = client
            .post("/get/prefetch")
            .cookie(cookie(user, false))
            .header(rocket::http::ContentType::JSON)
            .body(filter.to_string())
            .dispatch();
        assert_eq!(
            resp.status(),
            rocket::http::Status::Ok,
            "prefetch {user} default"
        );
        let body: serde_json::Value = resp.into_json().expect("prefetch json");
        assert_eq!(
            body["prefetch"]["dataLength"].as_u64(),
            Some(1),
            "{user} default must return only global root album"
        );
    }

    // opt-in: global + own personal root (never the other user's)
    for user in ["read-a", "read-b"] {
        let resp = client
            .post("/get/prefetch?include_personal=true")
            .cookie(cookie(user, false))
            .header(rocket::http::ContentType::JSON)
            .body(filter.to_string())
            .dispatch();
        assert_eq!(
            resp.status(),
            rocket::http::Status::Ok,
            "prefetch {user} opt-in"
        );
        let body: serde_json::Value = resp.into_json().expect("prefetch json");
        assert_eq!(
            body["prefetch"]["dataLength"].as_u64(),
            Some(2),
            "{user} opt-in must return global + own root album only"
        );
    }

    {
        let mut c = crate::model::config::APP_CONFIG
            .get()
            .expect("cfg")
            .write()
            .expect("lock");
        c.users_home = None;
    }
}

// ── B3: get_metadata personal-area read isolation ────────────────────

/// Shared setup: two users, one personal photo each (alice's tagged with a
/// unique tag), indexed. Returns alice/bob snapshot tokens + timestamps and
/// alice's personal asset id.
fn setup_personal_metadata() -> (
    rocket::local::blocking::Client,
    i64,
    String,
    String,
    i64,
    String,
) {
    let client = setup_image_home();
    let users_home = enable_users_home();
    seed("alice", false, None);
    seed("bob", false, None);

    let alice_dir = users_home.join("alice");
    let bob_dir = users_home.join("bob");
    std::fs::create_dir_all(&alice_dir).expect("a dir");
    std::fs::create_dir_all(&bob_dir).expect("b dir");
    let alice_photo = alice_dir.join("photo.jpg");
    let bob_photo = bob_dir.join("photo.jpg");
    snapfab::generate_batch(&[snapfab::PhotoSpec {
        output: Some(alice_photo.to_string_lossy().into()),
        format: Some("jpeg".into()),
        width: Some(8),
        height: Some(8),
        tags: Some(vec!["alice-secret-tag".to_string()]),
        exif_date: None,
        minimal: false,
    }])
    .expect("gen a");
    snapfab::generate_batch(&[snapfab::PhotoSpec {
        output: Some(bob_photo.to_string_lossy().into()),
        format: Some("jpeg".into()),
        width: Some(8),
        height: Some(8),
        tags: None,
        exif_date: None,
        minimal: false,
    }])
    .expect("gen b");

    crate::process::dir_album::reset_dir_album_cache();
    crate::tasks::actor::album_index::index_album_with_root("/", false, None)
        .expect("index accepted");
    wait_for_index_completion();

    let filter = json!({
        "And": [
            {"Not": {"ExtType": "album"}},
            {"Trashed": false}
        ]
    });

    // alice prefetch with personal opt-in to discover her asset id
    let resp = client
        .post("/get/prefetch?include_personal=true")
        .cookie(cookie("alice", false))
        .header(ContentType::JSON)
        .body(filter.to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "alice prefetch");
    let body: Value = resp.into_json().expect("alice prefetch json");
    let ts_alice = body["prefetch"]["timestamp"].as_i64().expect("ts");
    let token_alice = body["token"].as_str().expect("token").to_string();

    let resp = client
        .get(format!("/get/get-data?timestamp={ts_alice}&start=0&end=10"))
        .cookie(cookie("alice", false))
        .header(rocket::http::Header::new(
            "Authorization",
            format!("Bearer {token_alice}"),
        ))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "alice get-data");
    let data: Value = resp.into_json().expect("alice get-data json");
    let arr = data.as_array().expect("data array");
    assert_eq!(
        arr.len(),
        1,
        "expected one personal photo row for alice, got {}",
        arr.len()
    );
    let asset_alice = arr[0]["assetId"].as_str().expect("assetId").to_string();

    // bob prefetch (default, no personal) for a snapshot bearer + timestamp
    let resp = client
        .post("/get/prefetch")
        .cookie(cookie("bob", false))
        .header(ContentType::JSON)
        .body(filter.to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "bob prefetch");
    let body: Value = resp.into_json().expect("bob prefetch json");
    let ts_bob = body["prefetch"]["timestamp"].as_i64().expect("ts");
    let token_bob = body["token"].as_str().expect("token").to_string();

    (
        client,
        ts_alice,
        token_alice,
        asset_alice,
        ts_bob,
        token_bob,
    )
}

#[test]
fn b3_bob_snapshot_cannot_read_alice_personal_metadata() {
    let _guard = lock();
    let (client, _ts_alice, _token_alice, asset_alice, ts_bob, token_bob) =
        setup_personal_metadata();

    let resp = client
        .get(format!("/get/metadata/{asset_alice}?timestamp={ts_bob}"))
        .cookie(cookie("bob", false))
        .header(rocket::http::Header::new(
            "Authorization",
            format!("Bearer {token_bob}"),
        ))
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::NotFound,
        "bob must not read alice metadata"
    );

    let body = resp.into_string().unwrap_or_default();
    assert!(
        !body.contains("alice-secret-tag"),
        "alice's tag leaked to bob: {body}"
    );
    assert!(
        !body.contains("/users/alice"),
        "alice's path leaked to bob: {body}"
    );
}

#[test]
fn b3_alice_reads_own_personal_metadata_ok() {
    let _guard = lock();
    let (client, ts_alice, token_alice, asset_alice, _ts_bob, _token_bob) =
        setup_personal_metadata();

    let resp = client
        .get(format!("/get/metadata/{asset_alice}?timestamp={ts_alice}"))
        .cookie(cookie("alice", false))
        .header(rocket::http::Header::new(
            "Authorization",
            format!("Bearer {token_alice}"),
        ))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "alice must read own metadata");

    let body: Value = resp.into_json().expect("metadata json");
    assert_eq!(body["type"], "image");
    let tags = body["tags"].as_array().cloned().unwrap_or_default();
    let tag_strs: Vec<&str> = tags.iter().filter_map(|t| t.as_str()).collect();
    assert!(
        tag_strs.contains(&"alice-secret-tag"),
        "own tag missing: {tag_strs:?}"
    );
}
