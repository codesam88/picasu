use std::path::PathBuf;

use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::Value;

use crate::fixtures::auth::auth_cookie;

fn image_home() -> PathBuf {
    std::env::var("UROCISSA_IMAGE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // Fallback: resolve relative to the data directory used in tests.
            let data = std::env::var("UROCISSA_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/tmp"));
            data.join("images")
        })
}

pub fn discover_photo_hash(client: &Client, relative_path: &str) -> String {
    let image_home = image_home();
    let abs_path = image_home.join(relative_path);

    let cookie = auth_cookie(client);
    let body = serde_json::json!({"Path": abs_path.to_string_lossy()});

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
    assert!(
        data_length >= 1,
        "prefetch for {relative_path}: expected at least 1 result, got {data_length}"
    );
    let token = prefetch_body["token"]
        .as_str()
        .expect("prefetch.token")
        .to_owned();

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
    let data_body: Value = serde_json::from_slice(&data_resp.into_bytes().expect("get-data body"))
        .expect("valid get-data JSON");
    data_body[0]["abstractData"]["id"]
        .as_str()
        .expect("hash")
        .to_owned()
}

pub fn discover_album_id(client: &Client, relative_dir: &str) -> String {
    let image_home = image_home();
    let abs_dir = image_home.join(relative_dir);

    let cookie = auth_cookie(client);
    let albums_resp = client.get("/get/get-albums").cookie(cookie).dispatch();
    assert_eq!(albums_resp.status(), Status::Ok, "get-albums");
    let albums_body: Value =
        serde_json::from_slice(&albums_resp.into_bytes().expect("albums body"))
            .expect("valid albums JSON");
    let albums = albums_body.as_array().expect("albums array");
    let abs_dir_str = abs_dir.to_string_lossy();
    let album = albums
        .iter()
        .find(|a| a["dirPath"].as_str() == Some(&abs_dir_str))
        .unwrap_or_else(|| panic!("no album found for dir {abs_dir_str}"));
    album["albumId"].as_str().expect("albumId").to_owned()
}

/// Serve a compressed image via `/object/compressed/<hash_prefix>/<hash>.jpg`.
/// Handles the full token flow: admin auth -> Path-based prefetch ->
/// timestamp token -> get-data (hash token) -> serve image.
/// Returns the HTTP status code of the final image request.
pub fn serve_compressed_image(client: &Client, hash: &str) -> Status {
    let cookie = auth_cookie(client);
    let image_home = image_home();

    let body = serde_json::json!({"Path": image_home.to_string_lossy()});

    let prefetch_resp = client
        .post("/get/prefetch")
        .cookie(cookie.clone())
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    assert_eq!(
        prefetch_resp.status(),
        Status::Ok,
        "prefetch for serve_compressed_image"
    );
    let prefetch_body: Value =
        serde_json::from_slice(&prefetch_resp.into_bytes().expect("prefetch body"))
            .expect("valid prefetch JSON");
    let timestamp = prefetch_body["prefetch"]["timestamp"]
        .as_i64()
        .expect("prefetch.timestamp");
    let token = prefetch_body["token"]
        .as_str()
        .expect("prefetch.token")
        .to_owned();

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
        "get-data for serve_compressed_image"
    );
    let data_body: Value = serde_json::from_slice(&data_resp.into_bytes().expect("get-data body"))
        .expect("valid get-data JSON");
    let hash_token = data_body[0]["token"].as_str().expect("hash token");

    let hash_prefix = &hash[0..2];
    let resp = client
        .get(format!("/object/compressed/{hash_prefix}/{hash}.jpg"))
        .cookie(cookie.clone())
        .header(rocket::http::Header::new(
            "Authorization",
            format!("Bearer {hash_token}"),
        ))
        .dispatch();
    resp.status()
}
