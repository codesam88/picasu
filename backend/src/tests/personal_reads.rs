#![cfg(test)]

//! Personal-area read isolation: a user sees their own personal album and
//! never another user's, and the `users_home` root itself is never listed.

use serde_json::Value;

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
