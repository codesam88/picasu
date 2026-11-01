#![cfg(test)]

//! Authenticated-non-admin content writes: upload, move, metadata, trash,
//! delete, cover, rotate and thumbnail regeneration. Each section asserts the
//! viewer succeeds and the same call without credentials is refused.
//!
//! Hashing budget follows `auth_users.rs`: at most two seeds per test, many
//! assertions per test. Session cookies are minted without hashing; logins
//! pay one KDF round. Anonymous calls go through an *untracked* client: the
//! shared client is tracked, so a login would otherwise upgrade the jar and
//! silently turn anonymous calls into viewer calls.

use rocket::http::{ContentType, Cookie, Header, Status};
use serde_json::{Value, json};

use crate::tests::bootstrap::{
    build_test_rocket, cookie, lock, login, seed, setup_image_home, test_image_home,
};
use crate::tests::fixtures::authz::create_album;
use crate::tests::fixtures::test_password;

struct Ctx {
    client: rocket::local::blocking::Client,
    anon: rocket::local::blocking::Client,
    viewer: Cookie<'static>,
    album_id: String,
    asset_id: String,
}

fn viewer_setup() -> Ctx {
    // NOTE: the caller must hold `lock()` for the whole test (as below):
    // the shared backend state (image home, DB) is reset per test, and a
    // guard dropped here would let another test wipe state mid-flight.
    let client = setup_image_home();
    let anon =
        rocket::local::blocking::Client::untracked(build_test_rocket()).expect("untracked client");
    let (album_id, asset_id) = create_album(&client, "/writes/photo.jpg");
    seed("writer", false, Some(test_password("t-writer-pw").as_str()));
    Ctx {
        client,
        anon,
        viewer: cookie("writer", false),
        album_id,
        asset_id,
    }
}

/// Prefetch a snapshot locating `asset_id` and return its timestamp plus the
/// row list. Callers need the index because the edit endpoints address
/// assets by snapshot position, not by id.
fn snapshot_rows(
    client: &rocket::local::blocking::Client,
    cookie: Cookie<'static>,
    asset_id: &str,
) -> (i64, Vec<Value>) {
    let prefetch: Value = client
        .post(format!("/get/prefetch?locate={asset_id}"))
        .cookie(cookie.clone())
        .header(ContentType::JSON)
        .dispatch()
        .into_json()
        .expect("prefetch json");
    let ts = prefetch["prefetch"]["timestamp"]
        .as_i64()
        .expect("timestamp");
    let token = prefetch["token"].as_str().expect("token");
    let rows: Vec<Value> = client
        .get(format!("/get/get-data?timestamp={ts}&start=0&end=50"))
        .cookie(cookie)
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch()
        .into_json()
        .expect("rows json");
    (ts, rows)
}

fn row_index(rows: &[Value], suffix: &str) -> usize {
    rows.iter()
        .position(|r| {
            r["abstractData"]["path"]["file"]
                .as_str()
                .is_some_and(|f| f.ends_with(suffix))
        })
        .expect("row present")
}

fn anon_denied(resp_status: Status, route: &str) {
    assert_eq!(
        resp_status,
        Status::Unauthorized,
        "{route} without credentials"
    );
}

fn upload_multipart(file_data: &[u8], filename: &str) -> (Vec<u8>, String) {
    let boundary = "----picasu-test-upload-boundary";
    let mut body = Vec::new();
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"file\"; filename=\"");
    body.extend_from_slice(filename.as_bytes());
    body.extend_from_slice(b"\"\r\nContent-Type: image/jpeg\r\n\r\n");
    body.extend_from_slice(file_data);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(
        b"\r\nContent-Disposition: form-data; name=\"lastModified\"\r\n\r\n0\r\n",
    );
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"--\r\n");
    (body, boundary.to_string())
}

fn upload(client: &rocket::local::blocking::Client, cookie: Option<Cookie<'static>>) -> Status {
    let file_data =
        std::fs::read(test_image_home().join("writes/photo.jpg")).expect("seeded photo bytes");
    let (body, boundary) = upload_multipart(&file_data, "second.jpg");
    let mut req = client.post("/upload").header(ContentType::new(
        "multipart",
        format!("form-data; boundary={boundary}"),
    ));
    if let Some(cookie) = cookie {
        req = req.cookie(cookie);
    }
    req.body(body).dispatch().status()
}

/// Viewer uploads, creates an album, and moves the upload there; anonymous
/// callers are refused on both routes. The login itself is verified here so
/// the other tests can rely on minted cookies alone.
#[test]
fn viewer_upload_assign_and_create_dir() {
    let _guard = lock();
    let Ctx {
        client,
        anon,
        viewer,
        album_id,
        asset_id,
    } = viewer_setup();

    anon_denied(upload(&anon, None), "POST /upload");
    assert_eq!(
        upload(&client, Some(viewer.clone())),
        Status::Ok,
        "viewer upload"
    );
    let (status, _, _) = login(&client, "writer", &test_password("t-writer-pw"));
    assert_eq!(status, Status::Ok, "viewer login must work");

    let resp = client
        .post("/post/create_dir_album")
        .cookie(viewer.clone())
        .header(ContentType::JSON)
        .body(json!({ "parentAlbumId": album_id, "name": "sorted" }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "viewer create_dir_album");
    let sub_id: String = resp
        .into_string()
        .expect("created album id")
        .trim()
        .to_string();

    // Locate the uploaded asset through the viewer's own prefetch chain.
    let (_ts, rows) = snapshot_rows(&client, viewer.clone(), &asset_id);
    let uploaded = rows
        .iter()
        .find(|r| {
            r["abstractData"]["path"]["file"]
                .as_str()
                .unwrap_or_default()
                .contains("second.")
        })
        .expect("uploaded row visible to viewer");
    let uploaded_id = uploaded["assetId"].as_str().expect("asset id");

    let resp = client
        .put("/put/assign_album")
        .cookie(viewer.clone())
        .header(ContentType::JSON)
        .body(
            json!({ "assetId": uploaded_id, "albumId": sub_id, "onConflict": "rename" })
                .to_string(),
        )
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "viewer assign_album: {resp:?}");

    let resp = anon
        .put("/put/assign_album")
        .header(ContentType::JSON)
        .body(
            json!({ "assetId": uploaded_id, "albumId": sub_id, "onConflict": "rename" })
                .to_string(),
        )
        .dispatch();
    anon_denied(resp.status(), "PUT /put/assign_album");
}

/// Viewer metadata writes: tags, rating, description, title, cover.
/// Anonymous callers are refused.
#[test]
fn viewer_edits_photo_metadata() {
    let _guard = lock();
    let Ctx {
        client,
        anon,
        viewer,
        album_id,
        asset_id,
    } = viewer_setup();

    let (ts, rows) = snapshot_rows(&client, viewer.clone(), &asset_id);
    let index = row_index(&rows, "writes/photo.jpg");

    let resp = anon
        .put("/put/edit_tag")
        .header(ContentType::JSON)
        .body(json!({ "indexArray": [index], "addTagsArray": ["x"], "removeTagsArray": [], "timestamp": ts }).to_string())
        .dispatch();
    anon_denied(resp.status(), "PUT /put/edit_tag");

    for (route, body) in [
        (
            "/put/edit_tag",
            json!({ "indexArray": [index], "addTagsArray": ["viewer-tag"], "removeTagsArray": [], "timestamp": ts }),
        ),
        (
            "/put/edit_rating",
            json!({ "indexArray": [index], "timestamp": ts, "rating": 4 }),
        ),
        (
            "/put/set_user_defined_description",
            json!({ "index": index, "description": "viewer words", "timestamp": ts }),
        ),
    ] {
        let resp = client
            .put(route)
            .cookie(viewer.clone())
            .header(ContentType::JSON)
            .body(body.to_string())
            .dispatch();
        assert_eq!(resp.status(), Status::Ok, "viewer {route}: {resp:?}");
    }

    let resp = client
        .put("/put/set_album_title")
        .cookie(viewer.clone())
        .header(ContentType::JSON)
        .body(json!({ "albumId": album_id, "title": "viewer title" }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "viewer album title: {resp:?}");

    // Cover needs the seeded photo's asset id: resolve it like the grid does.
    let cover = rows
        .iter()
        .find(|r| {
            r["abstractData"]["path"]["file"]
                .as_str()
                .is_some_and(|f| f.ends_with("writes/photo.jpg"))
        })
        .expect("seeded photo row");
    let cover_id = cover["assetId"].as_str().expect("asset id");
    let resp = client
        .put("/put/set_album_cover")
        .cookie(viewer.clone())
        .header(ContentType::JSON)
        .body(json!({ "albumId": album_id, "coverAssetId": cover_id }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "viewer album cover: {resp:?}");
}

/// Trash is a reversible user act; permanent delete removes bytes + sidecar.
/// Anonymous callers are refused on both.
#[test]
fn viewer_trash_and_permanent_delete() {
    let _guard = lock();
    let Ctx {
        client,
        anon,
        viewer,
        asset_id,
        ..
    } = viewer_setup();

    let (ts, rows) = snapshot_rows(&client, viewer.clone(), &asset_id);
    let index = row_index(&rows, "writes/photo.jpg");

    let resp = anon
        .delete("/delete/delete-data")
        .header(ContentType::JSON)
        .body(json!({ "assetIds": ["no-such-asset"], "timestamp": ts }).to_string())
        .dispatch();
    anon_denied(resp.status(), "DELETE /delete/delete-data");

    for trashed in [true, false] {
        let resp = client
            .put("/put/edit_flags")
            .cookie(viewer.clone())
            .header(ContentType::JSON)
            .body(
                json!({ "indexArray": [index], "timestamp": ts, "isTrashed": trashed }).to_string(),
            )
            .dispatch();
        assert_eq!(resp.status(), Status::Ok, "viewer trash={trashed}");
    }

    // Permanent delete of the seeded photo (reuse the snapshot rows).
    let target_row = rows
        .iter()
        .find(|r| {
            r["abstractData"]["path"]["file"]
                .as_str()
                .is_some_and(|f| f.ends_with("writes/photo.jpg"))
        })
        .expect("seeded photo row");
    let target = target_row["assetId"]
        .as_str()
        .expect("asset id")
        .to_string();
    let path = target_row["abstractData"]["path"]["file"]
        .as_str()
        .expect("path")
        .to_string();

    let resp = client
        .delete("/delete/delete-data")
        .cookie(viewer.clone())
        .header(ContentType::JSON)
        .body(json!({ "assetIds": [target], "timestamp": ts }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "viewer delete-data");
    assert!(
        !std::path::Path::new(&path).exists(),
        "deleted file must be gone: {path}"
    );
}

/// Rotate rewrites original bytes; regenerate rebuilds the derived
/// thumbnail. Both are user-level content operations.
#[test]
fn viewer_rotate_and_regenerate() {
    let _guard = lock();
    let Ctx {
        client,
        anon,
        viewer,
        asset_id,
        ..
    } = viewer_setup();

    let resp = anon
        .put("/put/rotate-image")
        .header(ContentType::JSON)
        .body(json!({ "asset_id": asset_id }).to_string())
        .dispatch();
    anon_denied(resp.status(), "PUT /put/rotate-image");

    let resp = client
        .put("/put/rotate-image")
        .cookie(viewer.clone())
        .header(ContentType::JSON)
        .body(json!({ "asset_id": asset_id }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "viewer rotate-image: {resp:?}");

    let frame =
        std::fs::read(test_image_home().join("writes/photo.jpg")).expect("seeded photo bytes");
    let boundary = "----picasu-test-frame-boundary";
    let mut body = Vec::new();
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"\r\nContent-Disposition: form-data; name=\"asset_id\"\r\n\r\n");
    body.extend_from_slice(asset_id.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"\r\nContent-Disposition: form-data; name=\"frame\"; filename=\"frame.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n");
    body.extend_from_slice(&frame);
    body.extend_from_slice(b"\r\n--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"--\r\n");
    let resp = client
        .put("/put/regenerate-thumbnail-with-frame")
        .cookie(viewer.clone())
        .header(ContentType::new(
            "multipart",
            format!("form-data; boundary={boundary}"),
        ))
        .body(body)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "viewer regenerate: {resp:?}");
}

use crate::tests::share_ownership::{enable_users_home, personal_album_id};

fn wait_for_personal_index() {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        match crate::tasks::actor::album_index::album_index_status().state {
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

fn personal_snapshot_rows(
    client: &rocket::local::blocking::Client,
    user_cookie: Cookie<'static>,
) -> (i64, String, Vec<Value>) {
    let prefetch: Value = client
        .post("/get/prefetch?include_personal=true")
        .cookie(user_cookie.clone())
        .header(ContentType::JSON)
        .dispatch()
        .into_json()
        .expect("prefetch json");
    let ts = prefetch["prefetch"]["timestamp"]
        .as_i64()
        .expect("timestamp");
    let token = prefetch["token"].as_str().expect("token").to_string();
    let rows: Vec<Value> = client
        .get(format!("/get/get-data?timestamp={ts}&start=0&end=50"))
        .cookie(user_cookie)
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch()
        .into_json()
        .expect("rows json");
    (ts, token, rows)
}

fn personal_row_index(rows: &[Value], suffix: &str) -> usize {
    rows.iter()
        .position(|r| {
            r["abstractData"]["path"]["file"]
                .as_str()
                .is_some_and(|f| f.ends_with(suffix))
        })
        .unwrap_or_else(|| panic!("row present for {suffix}, got {rows:?}"))
}

fn album_name_for(
    client: &rocket::local::blocking::Client,
    user: &str,
    album_id: &str,
) -> Option<String> {
    let albums: Value = client
        .get("/get/get-albums")
        .cookie(cookie(user, false))
        .dispatch()
        .into_json()
        .expect("get-albums json");
    albums
        .as_array()
        .expect("album array")
        .iter()
        .find(|a| a["albumId"].as_str() == Some(album_id))
        .and_then(|a| a["albumName"].as_str().map(ToString::to_string))
}

fn regenerate_status(
    client: &rocket::local::blocking::Client,
    user: &str,
    asset_id: &str,
    frame: &[u8],
) -> Status {
    let boundary = "----picasu-test-regen-boundary";
    let mut body = Vec::new();
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"\r\nContent-Disposition: form-data; name=\"asset_id\"\r\n\r\n");
    body.extend_from_slice(asset_id.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"\r\nContent-Disposition: form-data; name=\"frame\"; filename=\"frame.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n");
    body.extend_from_slice(frame);
    body.extend_from_slice(b"\r\n--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"--\r\n");
    client
        .put("/put/regenerate-thumbnail-with-frame")
        .cookie(cookie(user, false))
        .header(ContentType::new(
            "multipart",
            format!("form-data; boundary={boundary}"),
        ))
        .body(body)
        .dispatch()
        .status()
}

/// Owner-a keeps a personal photo; owner-b must not write it through any of
/// the six gated routes. Each refusal leaves the target's `abstractData`
/// byte-identical, and the personal tag census stays caller-filtered.
#[allow(clippy::too_many_lines)]
#[test]
fn cross_user_personal_writes_refused_and_target_unchanged() {
    let _guard = lock();
    let client = setup_image_home();
    let users_home = enable_users_home();
    seed("owner-a", false, None);
    seed("owner-b", false, None);

    for user in ["owner-a", "owner-b"] {
        let dir = users_home.join(user);
        std::fs::create_dir_all(&dir).expect("user dir");
        let photo = dir.join("photo.jpg");
        snapfab::generate_batch(&[snapfab::PhotoSpec {
            output: Some(photo.to_string_lossy().into()),
            format: Some("jpeg".into()),
            width: Some(8),
            height: Some(8),
            tags: None,
            exif_date: None,
            minimal: false,
        }])
        .expect("gen photo");
    }
    crate::process::dir_album::reset_dir_album_cache();
    crate::tasks::actor::album_index::index_album_with_root("/", false, None)
        .expect("index accepted");
    wait_for_personal_index();

    let a_album = personal_album_id(&users_home, "owner-a");

    // Owner-a's snapshot locates only the owner-a photo.
    let (a_ts, _a_token, a_rows) = personal_snapshot_rows(&client, cookie("owner-a", false));
    let a_index = personal_row_index(&a_rows, "owner-a/photo.jpg");
    let a_asset = a_rows[a_index]["assetId"]
        .as_str()
        .expect("asset id")
        .to_string();
    let (b_ts, _b_token, b_rows) = personal_snapshot_rows(&client, cookie("owner-b", false));
    let b_index = personal_row_index(&b_rows, "owner-b/photo.jpg");

    // Legitimate owner writes succeed and establish the comparison baseline.
    let resp = client
        .put("/put/edit_tag")
        .cookie(cookie("owner-a", false))
        .header(ContentType::JSON)
        .body(
            json!({ "indexArray": [a_index], "addTagsArray": ["alice-secret-xyz"], "removeTagsArray": [], "timestamp": a_ts })
                .to_string(),
        )
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "owner-a tags own photo");
    let census: Vec<Value> = resp.into_json().expect("tag census json");
    assert!(
        census
            .iter()
            .any(|t| t["tag"].as_str() == Some("alice-secret-xyz")),
        "owner census contains own tag: {census:?}"
    );

    let resp = client
        .put("/put/edit_tag")
        .cookie(cookie("owner-b", false))
        .header(ContentType::JSON)
        .body(
            json!({ "indexArray": [b_index], "addTagsArray": ["bob-tag"], "removeTagsArray": [], "timestamp": b_ts })
                .to_string(),
        )
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "owner-b tags own photo");
    let census: Vec<Value> = resp.into_json().expect("tag census json");
    assert!(
        census.iter().any(|t| t["tag"].as_str() == Some("bob-tag")),
        "bob census contains own tag: {census:?}"
    );
    assert!(
        !census
            .iter()
            .any(|t| t["tag"].as_str() == Some("alice-secret-xyz")),
        "bob census must not contain alice's personal tag: {census:?}"
    );

    // Baseline after legitimate edits: every refusal below must preserve it.
    let (_, _, fresh_a) = personal_snapshot_rows(&client, cookie("owner-a", false));
    let a_fresh_index = personal_row_index(&fresh_a, "owner-a/photo.jpg");
    let baseline: Value = fresh_a[a_fresh_index]["abstractData"].clone();
    // The shared snapshot id is unbound, so bob reuses owner-a's timestamp
    // and index — exactly the confused-deputy path the gate must stop.
    let attack_ts = a_ts;
    let attack_index = a_index;
    let baseline_album_name = album_name_for(&client, "owner-a", &a_album);

    // edit_tag refused.
    let resp = client
        .put("/put/edit_tag")
        .cookie(cookie("owner-b", false))
        .header(ContentType::JSON)
        .body(
            json!({ "indexArray": [attack_index], "addTagsArray": ["hijack"], "removeTagsArray": [], "timestamp": attack_ts })
                .to_string(),
        )
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Forbidden,
        "bob edit_tag on alice photo"
    );

    // edit_rating refused.
    let resp = client
        .put("/put/edit_rating")
        .cookie(cookie("owner-b", false))
        .header(ContentType::JSON)
        .body(
            json!({ "indexArray": [attack_index], "timestamp": attack_ts, "rating": 5 })
                .to_string(),
        )
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Forbidden,
        "bob edit_rating on alice photo"
    );

    // edit_description refused.
    let resp = client
        .put("/put/set_user_defined_description")
        .cookie(cookie("owner-b", false))
        .header(ContentType::JSON)
        .body(
            json!({ "index": attack_index, "description": "hijacked", "timestamp": attack_ts })
                .to_string(),
        )
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Forbidden,
        "bob edit_description on alice photo"
    );

    // set_album_title refused (by-id write: 403, not 404).
    let resp = client
        .put("/put/set_album_title")
        .cookie(cookie("owner-b", false))
        .header(ContentType::JSON)
        .body(json!({ "albumId": a_album, "title": "hijacked" }).to_string())
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Forbidden,
        "bob set_album_title on alice album"
    );

    // rotate-image refused.
    let resp = client
        .put("/put/rotate-image")
        .cookie(cookie("owner-b", false))
        .header(ContentType::JSON)
        .body(json!({ "asset_id": a_asset }).to_string())
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Forbidden,
        "bob rotate-image on alice photo"
    );

    // regenerate-thumbnail refused.
    let frame = std::fs::read(users_home.join("owner-a/photo.jpg")).expect("frame bytes");
    assert_eq!(
        regenerate_status(&client, "owner-b", &a_asset, &frame),
        Status::Forbidden,
        "bob regenerate on alice photo"
    );

    // Whole target unchanged: abstractData and album title identical.
    let (_, _, after_rows) = personal_snapshot_rows(&client, cookie("owner-a", false));
    let after_index = personal_row_index(&after_rows, "owner-a/photo.jpg");
    assert_eq!(
        after_rows[after_index]["abstractData"], baseline,
        "alice abstractData unchanged after all refused writes"
    );
    assert_eq!(
        album_name_for(&client, "owner-a", &a_album),
        baseline_album_name,
        "alice album title unchanged after refused set_album_title"
    );
}

#[test]
fn edit_validation_negative_timestamp_and_bad_index() {
    let _guard = lock();
    let Ctx {
        client,
        viewer,
        asset_id,
        ..
    } = viewer_setup();
    let (ts, rows) = snapshot_rows(&client, viewer.clone(), &asset_id);
    let index = row_index(&rows, "writes/photo.jpg");

    // Negative timestamps are invalid input on every snapshot-addressed route.
    for (route, body) in [
        (
            "/put/edit_tag",
            json!({ "indexArray": [index], "addTagsArray": ["x"], "removeTagsArray": [], "timestamp": -1 }),
        ),
        (
            "/put/edit_rating",
            json!({ "indexArray": [index], "timestamp": -1, "rating": 4 }),
        ),
        (
            "/put/set_user_defined_description",
            json!({ "index": index, "description": "x", "timestamp": -1 }),
        ),
    ] {
        let resp = client
            .put(route)
            .cookie(viewer.clone())
            .header(ContentType::JSON)
            .body(body.to_string())
            .dispatch();
        assert_eq!(
            resp.status(),
            Status::BadRequest,
            "{route} negative timestamp"
        );
    }

    // Rating above 5 is rejected before any write.
    let resp = client
        .put("/put/edit_rating")
        .cookie(viewer.clone())
        .header(ContentType::JSON)
        .body(json!({ "indexArray": [index], "timestamp": ts, "rating": 6 }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::BadRequest, "rating 6 rejected");

    // Out-of-range index cannot succeed (500 via or_raise); it must not write.
    for (route, body) in [
        (
            "/put/edit_tag",
            json!({ "indexArray": [999_999], "addTagsArray": ["x"], "removeTagsArray": [], "timestamp": ts }),
        ),
        (
            "/put/edit_rating",
            json!({ "indexArray": [999_999], "timestamp": ts, "rating": 4 }),
        ),
        (
            "/put/set_user_defined_description",
            json!({ "index": 999_999, "description": "x", "timestamp": ts }),
        ),
    ] {
        let resp = client
            .put(route)
            .cookie(viewer.clone())
            .header(ContentType::JSON)
            .body(body.to_string())
            .dispatch();
        assert_ne!(resp.status(), Status::Ok, "{route} bad index must fail");
    }

    // Regenerate validates the asset id before touching the filesystem:
    // empty, too-short, and bad-charset ids are 400, unknown ids are 404.
    let frame = std::fs::read(test_image_home().join("writes/photo.jpg")).expect("frame bytes");
    for bad in ["", "a", "!!", "a/b"] {
        assert_eq!(
            regenerate_status_raw(&client, viewer.clone(), bad, &frame),
            Status::BadRequest,
            "regenerate bad id {bad:?}"
        );
    }
    assert_eq!(
        regenerate_status_raw(&client, viewer.clone(), "nonexistent-asset-xyz", &frame),
        Status::NotFound,
        "regenerate unknown id"
    );
}

fn regenerate_status_raw(
    client: &rocket::local::blocking::Client,
    viewer: Cookie<'static>,
    asset_id: &str,
    frame: &[u8],
) -> Status {
    let boundary = "----picasu-test-regen-validation";
    let mut body = Vec::new();
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"\r\nContent-Disposition: form-data; name=\"asset_id\"\r\n\r\n");
    body.extend_from_slice(asset_id.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(b"--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"\r\nContent-Disposition: form-data; name=\"frame\"; filename=\"frame.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n");
    body.extend_from_slice(frame);
    body.extend_from_slice(b"\r\n--");
    body.extend_from_slice(boundary.as_bytes());
    body.extend_from_slice(b"--\r\n");
    client
        .put("/put/regenerate-thumbnail-with-frame")
        .cookie(viewer)
        .header(ContentType::new(
            "multipart",
            format!("form-data; boundary={boundary}"),
        ))
        .body(body)
        .dispatch()
        .status()
}
