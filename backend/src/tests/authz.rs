#![cfg(test)]

//! Security regression tests for the share/token authorization invariants.
//! Each test reproduces a previously-vulnerable behavior end-to-end through
//! HTTP: it must fail against pre-fix code and pass once the invariant holds.
//!
//! These are integration tests (real Rocket `local::Client` over a tempdir),
//! not YAML scenarios: they need share creation and per-token minting
//! that the scenario DSL does not yet expose.

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

/// Serialize tests and reset shared backend state, matching the scenario
/// harness contract. `set_image_home` is required because the discovery
/// fixtures resolve paths through a module-level `OnceLock`, not the config.
fn setup() -> Client {
    let _ = &*TEST_ENV;
    reset_backend_state();
    let data = test_image_home();
    set_image_home(data);
    make_client()
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_SERIAL_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Generate one photo at `photo_rel`, index its directory, and return the
/// album id and the photo's asset id. The share in every test is created on
/// this album, so the photo must live directly in it (a child album would be
/// filtered out by the share's album expression).
fn create_album(client: &Client, photo_rel: &str) -> (String, String) {
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
    let index_path = format!("/{dir}");
    let resp = client
        .post("/post/index/album")
        .cookie(auth_cookie(client))
        .header(ContentType::JSON)
        .body(json!({ "album": index_path }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Accepted, "index {dir}");
    wait_for_album_index(client, 30_000);

    let album_id = discover_album_id(client, &dir);
    let asset_id = discover_asset_id(client, photo_rel.trim_start_matches('/'));
    (album_id, asset_id)
}

/// Create a share for `album_id` and return its id.
fn create_share(
    client: &Client,
    album_id: &str,
    show_metadata: bool,
    show_download: bool,
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
                "showUpload": false,
                "exp": 0
            })
            .to_string(),
        )
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "create_share");
    resp.into_string().expect("share id").trim().to_string()
}

/// `POST /get/prefetch` authenticated as a share, returning the snapshot id,
/// the `ClaimsTimestamp` bearer token, and the located row index (if any).
fn prefetch_as_share(
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

/// Admin `POST /get/prefetch`, returning snapshot id, token, and located index.
fn prefetch_as_admin(client: &Client, locate: &str) -> (i64, String, Option<usize>) {
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

/// The per-asset `ClaimsHash` token carried in a `get-data` row.
fn get_data_asset_token(client: &Client, ts: i64, snapshot_token: &str) -> String {
    let resp = client
        .get(format!("/get/get-data?timestamp={ts}&start=0&end=1"))
        .header(Header::new(
            "Authorization",
            format!("Bearer {snapshot_token}"),
        ))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "get-data");
    let body: Value = resp.into_json().expect("get-data json");
    body[0]["token"].as_str().expect("row token").to_string()
}

fn album_title(client: &Client, album_id: &str) -> Option<String> {
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

// ── JWT token-type confusion ────────────────────────────────────────────

/// A `ClaimsHash` asset token must not be accepted where a
/// `ClaimsTimestamp` snapshot token is required (metadata, get-data, renewal).
#[test]
fn asset_token_is_rejected_where_a_snapshot_token_is_required() {
    let _g = lock();
    let client = setup();
    let (album, asset) = create_album(&client, "/authz_asset_token/album/photo.jpg");
    let share = create_share(&client, &album, false, false);
    let (ts, snapshot_token, _) = prefetch_as_share(&client, &album, &share, Some(&asset));
    let asset_token = get_data_asset_token(&client, ts, &snapshot_token);

    // Metadata: the confused token must not satisfy GuardTimestamp.
    let resp = client
        .get(format!("/get/metadata/{asset}?timestamp={ts}"))
        .header(Header::new(
            "Authorization",
            format!("Bearer {asset_token}"),
        ))
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Unauthorized,
        "a ClaimsHash must not satisfy GuardTimestamp on get-metadata"
    );

    // get-data: the confused token must not mint fresh allow-original tokens.
    let resp = client
        .get(format!("/get/get-data?timestamp={ts}&start=0&end=1"))
        .header(Header::new(
            "Authorization",
            format!("Bearer {asset_token}"),
        ))
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Unauthorized,
        "a ClaimsHash must not satisfy GuardTimestamp on get-data"
    );

    // Renewal: a ClaimsHash must not be renewable as a snapshot token.
    let resp = client
        .post("/post/renew-timestamp-token")
        .header(Header::new("x-album-id", album.clone()))
        .header(Header::new("x-share-id", share.clone()))
        .header(ContentType::JSON)
        .body(json!({ "token": asset_token }).to_string())
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Unauthorized,
        "a ClaimsHash must not be renewable as a ClaimsTimestamp"
    );
}

/// A `ClaimsTimestamp` snapshot token must not be accepted where a
/// `ClaimsHash` asset token is required. This direction already fails closed
/// (ClaimsHash's required fields are absent), so it is a regression pin rather
/// than a reproduction.
#[test]
fn snapshot_token_is_rejected_where_an_asset_token_is_required() {
    let _g = lock();
    let client = setup();
    let (album, asset) = create_album(&client, "/authz_snapshot_token/album/photo.jpg");
    let share = create_share(&client, &album, true, true);
    let (_, snapshot_token, _) = prefetch_as_share(&client, &album, &share, Some(&asset));

    let resp = client
        .get(format!("/object/compressed/aa/{asset}.jpg"))
        .header(Header::new("x-album-id", album.clone()))
        .header(Header::new("x-share-id", share.clone()))
        .header(Header::new(
            "Authorization",
            format!("Bearer {snapshot_token}"),
        ))
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Unauthorized,
        "a ClaimsTimestamp must not satisfy GuardHash"
    );
}

// ── get-metadata is album-scoped ────────────────────────────────────

/// A share on AlbumB must not read AlbumA's asset metadata by id.
#[test]
fn metadata_is_scoped_to_the_shares_album() {
    let _g = lock();
    let client = setup();
    let (_album_a, asset_a) = create_album(&client, "/authz_scope_a/album/photo.jpg");
    let (album_b, _asset_b) = create_album(&client, "/authz_scope_b/album/photo.jpg");
    let share_b = create_share(&client, &album_b, true, false);
    let (ts, token_b, _) = prefetch_as_share(&client, &album_b, &share_b, None);

    let resp = client
        .get(format!("/get/metadata/{asset_a}?timestamp={ts}"))
        .header(Header::new("Authorization", format!("Bearer {token_b}")))
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::NotFound,
        "a share must not read another album's asset metadata"
    );
}

// ── share-guarded writes are bound to the caller ────────────────────

/// A share on AlbumB must not rename AlbumA.
#[test]
fn share_cannot_rename_another_album() {
    let _g = lock();
    let client = setup();
    let (album_a, _asset_a) = create_album(&client, "/authz_rename_a/album/photo.jpg");
    let (album_b, _asset_b) = create_album(&client, "/authz_rename_b/album/photo.jpg");
    let share_b = create_share(&client, &album_b, true, false);
    let before = album_title(&client, &album_a);

    let resp = client
        .put("/put/set_album_title")
        .header(Header::new("x-album-id", album_b.clone()))
        .header(Header::new("x-share-id", share_b.clone()))
        .header(ContentType::JSON)
        .body(json!({ "albumId": album_a.clone(), "title": "pwned" }).to_string())
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Forbidden,
        "a share must not rename another album"
    );
    assert_eq!(
        album_title(&client, &album_a),
        before,
        "a refused rename must leave the target album unchanged"
    );
}

/// A share on AlbumA must not write a description into AlbumB's asset by
/// supplying AlbumB's snapshot timestamp.
#[test]
fn share_cannot_write_another_albums_asset() {
    let _g = lock();
    let client = setup();
    let (album_a, _asset_a) = create_album(&client, "/authz_desc_a/album/photo.jpg");
    let (_album_b, asset_b) = create_album(&client, "/authz_desc_b/album/photo.jpg");
    let share_a = create_share(&client, &album_a, true, false);
    let (ts_b, _, idx_b) = prefetch_as_admin(&client, &asset_b);
    let idx_b = idx_b.expect("album B asset must be locatable");

    let resp = client
        .put("/put/set_user_defined_description")
        .header(Header::new("x-album-id", album_a.clone()))
        .header(Header::new("x-share-id", share_a.clone()))
        .header(ContentType::JSON)
        .body(json!({ "index": idx_b, "description": "pwned", "timestamp": ts_b }).to_string())
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Forbidden,
        "a share must not write another album's asset"
    );
}

// ── token renewal is bound to the presenter's share ─────────────────

/// ShareA's credentials must not renew ShareB's snapshot token.
#[test]
fn renewal_is_bound_to_the_presenters_share() {
    let _g = lock();
    let client = setup();
    let (album_a, _asset_a) = create_album(&client, "/authz_renew_a/album/photo.jpg");
    let (album_b, _asset_b) = create_album(&client, "/authz_renew_b/album/photo.jpg");
    let share_a = create_share(&client, &album_a, true, false);
    let share_b = create_share(&client, &album_b, true, false);
    let (_, token_b, _) = prefetch_as_share(&client, &album_b, &share_b, None);

    let resp = client
        .post("/post/renew-timestamp-token")
        .header(Header::new("x-album-id", album_a.clone()))
        .header(Header::new("x-share-id", share_a.clone()))
        .header(ContentType::JSON)
        .body(json!({ "token": token_b }).to_string())
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Unauthorized,
        "a share must not renew another share's token"
    );
}

/// renewal must re-validate the embedded share, so a share that has since
/// expired stops yielding refreshed tokens.
#[test]
fn renewal_revalidates_the_embedded_share() {
    let _g = lock();
    let client = setup();
    let (album, asset) = create_album(&client, "/authz_renew_expired/album/photo.jpg");
    let share = create_share(&client, &album, true, false);
    let (_, token, _) = prefetch_as_share(&client, &album, &share, Some(&asset));

    // Expire the share after the token was minted.
    let share_obj = json!({
        "url": share,
        "description": "authz test",
        "password": null,
        "showMetadata": true,
        "showDownload": false,
        "showUpload": false,
        "exp": chrono::Utc::now().timestamp() - 3600
    });
    let resp = client
        .put("/put/edit_share")
        .cookie(auth_cookie(&client))
        .header(ContentType::JSON)
        .body(json!({ "albumId": album, "share": share_obj }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "expire share");

    // Admin renewal of the now-expired share's token must be refused.
    let resp = client
        .post("/post/renew-timestamp-token")
        .cookie(auth_cookie(&client))
        .header(ContentType::JSON)
        .body(json!({ "token": token }).to_string())
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Unauthorized,
        "renewal must re-validate the embedded share"
    );
}
