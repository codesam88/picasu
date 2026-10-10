//! Shared helpers for the authorization test suites (`tests::authz` and
//! `tests::authz_props`): a reset client, album/photo setup, share creation,
//! and the prefetch/get-data token flows.

use rocket::http::{ContentType, Header, Status};
use rocket::local::blocking::Client;
use serde_json::{Value, json};
use snapfab::{PhotoSpec, generate_batch};

use crate::tests::bootstrap::{
    TEST_ENV, TEST_SERIAL_GUARD, make_client, reset_backend_state, test_image_home,
};
use crate::tests::fixtures::{
    auth_cookie, discover_album_id, discover_asset_id, set_image_home, wait_for_album_index,
};

/// Serialize a test against the shared backend state.
pub fn lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_SERIAL_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Reset shared state and return a fresh client. `set_image_home` is required
/// because the discovery fixtures resolve paths through a module-level
/// `OnceLock`, not the config.
pub fn setup() -> Client {
    let _ = &*TEST_ENV;
    reset_backend_state();
    let data = test_image_home();
    set_image_home(data);
    make_client()
}

/// Generate one photo at `photo_rel`, index its directory, and return the
/// album id and the photo's asset id. The album is the photo's own directory,
/// so a share created on it is not filtered out by its album expression.
pub fn create_album(client: &Client, photo_rel: &str) -> (String, String) {
    let data = test_image_home();
    let photo_abs = data.join(photo_rel.trim_start_matches('/'));
    std::fs::create_dir_all(photo_abs.parent().expect("photo parent")).expect("create dir");
    generate_batch(&[PhotoSpec {
        output: Some(photo_abs.to_string_lossy().into_owned()),
        format: Some("jpeg".into()),
        width: Some(8),
        height: Some(8),
        tags: None,
        exif_date: None,
        minimal: false,
    }])
    .expect("generate photo");

    let dir = photo_abs
        .parent()
        .expect("photo parent")
        .strip_prefix(&data)
        .expect("photo under image home")
        .to_string_lossy()
        .into_owned();
    let resp = client
        .post("/post/index/album")
        .cookie(auth_cookie(client))
        .header(ContentType::JSON)
        .body(json!({ "album": format!("/{dir}") }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Accepted, "index {dir}");
    wait_for_album_index(client, 30_000);

    let album_id = discover_album_id(client, &dir);
    let asset_id = discover_asset_id(client, photo_rel.trim_start_matches('/'));
    (album_id, asset_id)
}

/// Create a share on `album_id` with the given policy and return its id.
pub fn create_share(
    client: &Client,
    album_id: &str,
    show_metadata: bool,
    show_download: bool,
    show_upload: bool,
) -> String {
    let resp = client
        .post("/post/create_share")
        .cookie(auth_cookie(client))
        .header(ContentType::JSON)
        .body(
            json!({
                "albumId": album_id,
                "description": "authz test",
                "password": null,
                "showMetadata": show_metadata,
                "showDownload": show_download,
                "showUpload": show_upload,
                "exp": 0
            })
            .to_string(),
        )
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "create_share");
    resp.into_string().expect("share id").trim().to_string()
}

/// `POST /get/prefetch` authenticated as a share. Returns the snapshot id, the
/// `ClaimsTimestamp` token, and the located row index (if any).
pub fn prefetch_as_share(
    client: &Client,
    album_id: &str,
    share_id: &str,
    locate: Option<&str>,
) -> (i64, String, Option<usize>) {
    let url = locate.map_or_else(
        || "/get/prefetch".to_string(),
        |asset| format!("/get/prefetch?locate={asset}"),
    );
    let resp = client
        .post(url)
        .header(Header::new("x-album-id", album_id.to_string()))
        .header(Header::new("x-share-id", share_id.to_string()))
        .header(ContentType::JSON)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "prefetch as share");
    let body: Value = resp.into_json().expect("prefetch json");
    (
        body["prefetch"]["timestamp"].as_i64().expect("timestamp"),
        body["token"].as_str().expect("token").to_string(),
        body["prefetch"]["locateTo"].as_u64().map(|v| v as usize),
    )
}

/// Admin `POST /get/prefetch`. Returns snapshot id, token, located index.
pub fn prefetch_as_admin(client: &Client, locate: &str) -> (i64, String, Option<usize>) {
    let resp = client
        .post(format!("/get/prefetch?locate={locate}"))
        .cookie(auth_cookie(client))
        .header(ContentType::JSON)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "prefetch as admin");
    let body: Value = resp.into_json().expect("prefetch json");
    (
        body["prefetch"]["timestamp"].as_i64().expect("timestamp"),
        body["token"].as_str().expect("token").to_string(),
        body["prefetch"]["locateTo"].as_u64().map(|v| v as usize),
    )
}

/// The rows of one `get-data` page.
pub fn get_data_rows(client: &Client, ts: i64, snapshot_token: &str, end: usize) -> Vec<Value> {
    let resp = client
        .get(format!("/get/get-data?timestamp={ts}&start=0&end={end}"))
        .header(Header::new(
            "Authorization",
            format!("Bearer {snapshot_token}"),
        ))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "get-data");
    resp.into_json().expect("get-data json")
}

/// The per-asset `ClaimsHash` token in the first `get-data` row.
pub fn get_data_asset_token(client: &Client, ts: i64, snapshot_token: &str) -> String {
    get_data_rows(client, ts, snapshot_token, 1)[0]["token"]
        .as_str()
        .expect("row token")
        .to_string()
}

/// The stored display title of an album, read as admin.
pub fn album_title(client: &Client, album_id: &str) -> Option<String> {
    let resp = client
        .get("/get/get-albums")
        .cookie(auth_cookie(client))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "get-albums");
    let body: Value = resp.into_json().expect("albums json");
    body.as_array()
        .expect("albums array")
        .iter()
        .find(|a| a["albumId"].as_str() == Some(album_id))
        .and_then(|a| a["albumName"].as_str().map(ToString::to_string))
}
