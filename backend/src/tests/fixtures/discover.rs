use std::path::PathBuf;
use std::sync::OnceLock;

use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::Value;

use super::auth::auth_cookie;

static IMAGE_HOME: OnceLock<PathBuf> = OnceLock::new();

pub fn set_image_home(path: PathBuf) {
    IMAGE_HOME.set(path).ok();
}

fn image_home() -> PathBuf {
    IMAGE_HOME
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("/tmp/images"))
}

pub fn discover_photo_hash(client: &Client, relative_path: &str) -> String {
    let image_home = image_home();
    let abs_path = image_home.join(relative_path);

    let cookie = auth_cookie(client);
    let body = serde_json::json!({"Path": abs_path.to_string_lossy()});

    // Uploads update the tree snapshot through a detached batch, so a freshly
    // uploaded path may not be locatable yet. Poll until the path shows up.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(10_000);
    let mut found: Option<String> = None;
    while std::time::Instant::now() < deadline {
        let prefetch_resp = client
            .post("/get/prefetch")
            .cookie(cookie.clone())
            .header(ContentType::JSON)
            .body(body.to_string())
            .dispatch();
        assert_eq!(
            prefetch_resp.status(),
            Status::Ok,
            "prefetch for {relative_path}: expected 200"
        );
        let prefetch_body: Value =
            serde_json::from_slice(&prefetch_resp.into_bytes().expect("prefetch body"))
                .expect("valid prefetch JSON");
        let timestamp = prefetch_body["prefetch"]["timestamp"]
            .as_i64()
            .expect("prefetch.timestamp");
        let data_length = prefetch_body["prefetch"]["dataLength"]
            .as_u64()
            .expect("prefetch.dataLength");
        let token = prefetch_body["token"]
            .as_str()
            .expect("prefetch.token")
            .to_owned();

        if data_length >= 1 {
            let data_resp = client
                .get(format!("/get/get-data?timestamp={timestamp}&start=0&end=1"))
                .header(rocket::http::Header::new(
                    "Authorization",
                    format!("Bearer {token}"),
                ))
                .dispatch();
            assert_eq!(
                data_resp.status(),
                Status::Ok,
                "get-data for {relative_path}"
            );
            let data_body: Value =
                serde_json::from_slice(&data_resp.into_bytes().expect("get-data body"))
                    .expect("valid get-data JSON");
            found = data_body[0]["abstractData"]["id"]
                .as_str()
                .map(|s| s.to_owned());
            if found.is_some() {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    found.unwrap_or_else(|| panic!("prefetch for {relative_path}: path not locatable within 10s"))
}

/// Discover the `asset_id` for a file by its relative path.
/// Uses `Path` expression filter to find the specific file, then captures
/// the `asset_id` from the `get-data` response.
pub fn discover_asset_id(client: &Client, relative_path: &str) -> String {
    let image_home = image_home();
    let abs_path = image_home.join(relative_path);

    // DEBUG: uncomment to trace asset_id discovery
    // eprintln!("DEBUG discover_asset_id: {relative_path}");

    let cookie = auth_cookie(client);
    let body = serde_json::json!({"Path": abs_path.to_string_lossy()});

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(10_000);
    let mut found: Option<String> = None;
    while std::time::Instant::now() < deadline {
        let prefetch_resp = client
            .post("/get/prefetch")
            .cookie(cookie.clone())
            .header(ContentType::JSON)
            .body(body.to_string())
            .dispatch();
        assert_eq!(
            prefetch_resp.status(),
            Status::Ok,
            "prefetch for {relative_path}: expected 200"
        );
        let prefetch_body: Value =
            serde_json::from_slice(&prefetch_resp.into_bytes().expect("prefetch body"))
                .expect("valid prefetch JSON");
        let timestamp = prefetch_body["prefetch"]["timestamp"]
            .as_i64()
            .expect("prefetch.timestamp");
        let data_length = prefetch_body["prefetch"]["dataLength"]
            .as_u64()
            .expect("prefetch.dataLength");
        let token = prefetch_body["token"]
            .as_str()
            .expect("prefetch.token")
            .to_owned();

        if data_length >= 1 {
            let data_resp = client
                .get(format!("/get/get-data?timestamp={timestamp}&start=0&end=1"))
                .header(rocket::http::Header::new(
                    "Authorization",
                    format!("Bearer {token}"),
                ))
                .dispatch();
            assert_eq!(
                data_resp.status(),
                Status::Ok,
                "get-data for {relative_path}"
            );
            let data_body: Value =
                serde_json::from_slice(&data_resp.into_bytes().expect("get-data body"))
                    .expect("valid get-data JSON");
            // Try to get asset_id from the response (new field).
            found = data_body[0]["assetId"].as_str().map(|s| s.to_owned());
            if found.is_some() {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    found.unwrap_or_else(|| {
        panic!("prefetch for {relative_path}: asset_id not locatable within 10s")
    })
}

pub fn discover_album_id(client: &Client, relative_dir: &str) -> String {
    let cookie = auth_cookie(client);
    let albums_resp = client.get("/get/get-albums").cookie(cookie).dispatch();
    assert_eq!(albums_resp.status(), Status::Ok, "get-albums");
    let albums_body: Value =
        serde_json::from_slice(&albums_resp.into_bytes().expect("albums body"))
            .expect("valid albums JSON");
    let albums = albums_body.as_array().expect("albums array");
    let album = albums
        .iter()
        .find(|a| a["dirPath"].as_str() == Some(relative_dir))
        .unwrap_or_else(|| panic!("no album found for dir {relative_dir}"));
    album["albumId"].as_str().expect("albumId").to_owned()
}
