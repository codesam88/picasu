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

use crate::router::auth::{ClaimsAsset, ClaimsSnapshot, decode_typed};
use crate::tests::bootstrap::{lock, seed, setup_image_home, test_image_home};
use crate::tests::fixtures::{
    auth_cookie, discover_album_id, discover_asset_id, wait_for_album_index,
};
use crate::tests::share_ownership::enable_users_home;

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
/// the `ClaimsSnapshot` bearer token, and the located row index (if any).
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
        body["prefetch"]["locateTo"]
            .as_u64()
            .map(|v| usize::try_from(v).expect("prefetch locateTo fits in usize")),
    )
}

/// The per-asset `ClaimsAsset` token carried in a `get-data` row.
fn get_data_asset_token(
    client: &Client,
    ts: i64,
    snapshot_token: &str,
    album: &str,
    share: &str,
) -> String {
    let resp = client
        .get(format!("/get/get-data?timestamp={ts}&start=0&end=1"))
        .header(Header::new("x-album-id", album.to_string()))
        .header(Header::new("x-share-id", share.to_string()))
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

/// A `ClaimsAsset` asset token must not be accepted where a
/// `ClaimsSnapshot` snapshot token is required (metadata, get-data, renewal).
#[test]
fn asset_token_is_rejected_where_a_snapshot_token_is_required() {
    let _g = lock();
    let client = setup_image_home();
    let (album, asset) = create_album(&client, "/authz_asset_token/album/photo.jpg");
    let share = create_share(&client, &album, false, false);
    let (ts, snapshot_token, _) = prefetch_as_share(&client, &album, &share, Some(&asset));
    let asset_token = get_data_asset_token(&client, ts, &snapshot_token, &album, &share);

    // Metadata: the confused token must not satisfy GuardSnapshot.
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
        "a ClaimsAsset must not satisfy GuardSnapshot on get-metadata"
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
        "a ClaimsAsset must not satisfy GuardSnapshot on get-data"
    );

    // Renewal: a ClaimsAsset must not be renewable as a snapshot token.
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
        "a ClaimsAsset must not be renewable as a ClaimsSnapshot"
    );
}

/// A `ClaimsSnapshot` snapshot token must not be accepted where a
/// `ClaimsAsset` asset token is required. This direction already fails closed
/// (`ClaimsAsset`'s required fields are absent), so it is a regression pin rather
/// than a reproduction.
#[test]
fn snapshot_token_is_rejected_where_an_asset_token_is_required() {
    let _g = lock();
    let client = setup_image_home();
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
        "a ClaimsSnapshot must not satisfy GuardHash"
    );
}

// ── F10: query-string bearer and shared-cache headers ─────────────────────

/// A valid bearer token in the query string must be refused: `?token=` leaks
/// credentials into proxy logs, browser history and `Referer` headers, and
/// every client here controls the `Authorization` header instead. The control
/// leg proves the token itself is good, so the 401 is about the transport.
#[test]
fn a_valid_bearer_token_in_the_query_is_refused() {
    let _g = lock();
    let client = setup_image_home();
    let (album, asset) = create_album(&client, "/authz_query_token/album/photo.jpg");
    let share = create_share(&client, &album, false, false);
    let (ts, snapshot_token, _) = prefetch_as_share(&client, &album, &share, Some(&asset));

    let resp = client
        .get(format!("/get/get-data?timestamp={ts}&start=0&end=1"))
        .header(Header::new("x-album-id", album.clone()))
        .header(Header::new("x-share-id", share.clone()))
        .header(Header::new(
            "Authorization",
            format!("Bearer {snapshot_token}"),
        ))
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Ok,
        "control: the header form authenticates"
    );

    let resp = client
        .get(format!(
            "/get/get-data?timestamp={ts}&start=0&end=1&token={snapshot_token}"
        ))
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Unauthorized,
        "the same token in the query string must not authenticate"
    );
}

/// Authorized media is marked `Cache-Control: private`: `public` lets a
/// shared cache retain a response and re-serve it to a later unauthenticated
/// request for the same URL (RFC 9111).
#[test]
fn media_responses_are_not_shared_cacheable() {
    let _g = lock();
    let client = setup_image_home();
    let (album, asset) = create_album(&client, "/authz_cache_header/album/photo.jpg");
    let share = create_share(&client, &album, true, true);
    let (ts, snapshot_token, _) = prefetch_as_share(&client, &album, &share, Some(&asset));
    let asset_token = get_data_asset_token(&client, ts, &snapshot_token, &album, &share);

    let validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    let claims: crate::router::auth::ClaimsAsset =
        crate::router::auth::decode_typed(&asset_token, &validation).expect("decode asset token");
    let hash = claims.hash.to_string();
    let resp = client
        .get(format!("/object/compressed/{}/{}.jpg", &hash[0..2], hash))
        .header(Header::new("x-album-id", album))
        .header(Header::new("x-share-id", share))
        .header(Header::new(
            "Authorization",
            format!("Bearer {asset_token}"),
        ))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "media serving");
    let cache_control = resp
        .headers()
        .get_one("Cache-Control")
        .expect("Cache-Control on media responses");
    assert!(
        cache_control.contains("private") && !cache_control.contains("public"),
        "media must be private for shared caches, got {cache_control}"
    );
}

// ── original serving requires allow_original ──────────────────────────────

/// A bearer token with `allow_original=false` must be denied with 401 on
/// the original-file route. Pre-fix the guard `Forward`s into the SPA
/// catch-all instead of answering `Error`, masking the denial as a
/// non-401.
#[test]
fn original_serving_without_allow_original_is_401() {
    let _g = lock();
    let client = setup_image_home();
    let asset: arrayvec::ArrayString<64> = "a".repeat(64).parse().expect("asset id");
    let hash: arrayvec::ArrayString<64> = "b".repeat(64).parse().expect("hash");
    let token = crate::router::auth::ClaimsAsset::new(
        hash,
        asset,
        chrono::Utc::now().timestamp_millis(),
        false,
    )
    .encode();
    let resp = client
        .get(format!("/object/imported/aa/{asset}.jpg"))
        .cookie(auth_cookie(&client))
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Unauthorized,
        "a token without original access must be denied with 401, not forwarded"
    );
}

// ── get-metadata is album-scoped ────────────────────────────────────

/// A share on `AlbumB` must not read `AlbumA`'s asset metadata by id.
#[test]
fn metadata_is_scoped_to_the_shares_album() {
    let _g = lock();
    let client = setup_image_home();
    let (_album_a, asset_a) = create_album(&client, "/authz_scope_a/album/photo.jpg");
    let (album_b, _asset_b) = create_album(&client, "/authz_scope_b/album/photo.jpg");
    let share_b = create_share(&client, &album_b, true, false);
    let (ts, token_b, _) = prefetch_as_share(&client, &album_b, &share_b, None);

    let resp = client
        .get(format!("/get/metadata/{asset_a}?timestamp={ts}"))
        .header(Header::new("x-album-id", album_b.clone()))
        .header(Header::new("x-share-id", share_b.clone()))
        .header(Header::new("Authorization", format!("Bearer {token_b}")))
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::NotFound,
        "a share must not read another album's asset metadata"
    );
}

// ── shares are read-only: share credentials write nothing ───────────────

/// A share may not rename albums — not even its own. Share writes are
/// rejected at the guard with 401; the album is untouched.
#[test]
fn share_cannot_rename_any_album() {
    let _g = lock();
    let client = setup_image_home();
    let (album_a, _asset_a) = create_album(&client, "/authz_rename_a/album/photo.jpg");
    let (_album_b, _asset_b) = create_album(&client, "/authz_rename_b/album/photo.jpg");
    let share_a = create_share(&client, &album_a, true, false);
    let before = album_title(&client, &album_a);

    let resp = client
        .put("/put/set_album_title")
        .header(Header::new("x-album-id", album_a.clone()))
        .header(Header::new("x-share-id", share_a.clone()))
        .header(ContentType::JSON)
        .body(json!({ "albumId": album_a.clone(), "title": "pwned" }).to_string())
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Unauthorized,
        "a share must not rename any album, including its own"
    );
    assert_eq!(
        album_title(&client, &album_a),
        before,
        "a refused rename must leave the target album unchanged"
    );
}

/// A share may not write asset descriptions — not even into its own album.
#[test]
fn share_cannot_write_any_albums_asset() {
    let _g = lock();
    let client = setup_image_home();
    let (album_a, asset_a) = create_album(&client, "/authz_desc_a/album/photo.jpg");
    let (_album_b, _asset_b) = create_album(&client, "/authz_desc_b/album/photo.jpg");
    let share_a = create_share(&client, &album_a, true, false);
    let (ts_a, _, idx_a) = prefetch_as_share(&client, &album_a, &share_a, Some(&asset_a));
    let idx_a = idx_a.expect("album A asset must be locatable");

    let resp = client
        .put("/put/set_user_defined_description")
        .header(Header::new("x-album-id", album_a.clone()))
        .header(Header::new("x-share-id", share_a.clone()))
        .header(ContentType::JSON)
        .body(json!({ "index": idx_a, "description": "pwned", "timestamp": ts_a }).to_string())
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Unauthorized,
        "a share must not write any asset description, including its own album's"
    );
}

// ── token renewal is bound to the presenter's share ─────────────────

/// `ShareA`'s credentials must not renew `ShareB`'s snapshot token.
#[test]
fn renewal_is_bound_to_the_presenters_share() {
    let _g = lock();
    let client = setup_image_home();
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
    let client = setup_image_home();
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

#[test]
fn prefetch_snapshot_token_carries_caller() {
    let _g = lock();
    let client = setup_image_home();
    let (album, asset) = create_album(&client, "/caller_prefetch/album/photo.jpg");
    let share = create_share(&client, &album, false, false);

    // User prefetch: the snapshot token carries the requesting user's id.
    let resp = client
        .post("/get/prefetch")
        .cookie(auth_cookie(&client))
        .header(ContentType::JSON)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "prefetch as user");
    let body: Value = resp.into_json().expect("prefetch json");
    let user_token = body["token"].as_str().expect("token").to_string();
    let decoded_user: ClaimsSnapshot =
        decode_typed(&user_token, &crate::router::auth::VALIDATION).expect("decode user snapshot");
    assert_eq!(decoded_user.caller, "admin");

    // Share prefetch: the snapshot token carries an empty caller.
    let (_, share_token, _) = prefetch_as_share(&client, &album, &share, Some(&asset));
    let decoded_share: ClaimsSnapshot =
        decode_typed(&share_token, &crate::router::auth::VALIDATION)
            .expect("decode share snapshot");
    assert_eq!(decoded_share.caller, "");
}

// ── B2: snapshot tokens are bound to the presenter (caller) ─────────

/// Bob presenting his own session cannot reuse Alice's snapshot bearer.
#[test]
fn b2_cross_user_reuse_get_data_is_401() {
    let _g = lock();
    let client = setup_image_home();
    let (_album, _asset) = create_album(&client, "/b2_cross_data/album/photo.jpg");
    crate::tests::bootstrap::seed("alice", false, None);
    crate::tests::bootstrap::seed("bob", false, None);

    let resp = client
        .post("/get/prefetch")
        .cookie(crate::tests::bootstrap::cookie("alice", false))
        .header(ContentType::JSON)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "alice prefetch");
    let body: Value = resp.into_json().expect("prefetch json");
    let ts = body["prefetch"]["timestamp"].as_i64().expect("timestamp");
    let alice_token = body["token"].as_str().expect("token").to_string();

    let resp = client
        .get(format!("/get/get-data?timestamp={ts}&start=0&end=1"))
        .cookie(crate::tests::bootstrap::cookie("bob", false))
        .header(Header::new(
            "Authorization",
            format!("Bearer {alice_token}"),
        ))
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Unauthorized,
        "bob must not reuse alice's snapshot on get-data"
    );
}

#[test]
fn b2_cross_user_reuse_get_rows_and_scroll_bar_is_401() {
    let _g = lock();
    let client = setup_image_home();
    let (_album, _asset) = create_album(&client, "/b2_cross_rows/album/photo.jpg");
    crate::tests::bootstrap::seed("alice", false, None);
    crate::tests::bootstrap::seed("bob", false, None);

    let resp = client
        .post("/get/prefetch")
        .cookie(crate::tests::bootstrap::cookie("alice", false))
        .header(ContentType::JSON)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "alice prefetch");
    let body: Value = resp.into_json().expect("prefetch json");
    let ts = body["prefetch"]["timestamp"].as_i64().expect("timestamp");
    let alice_token = body["token"].as_str().expect("token").to_string();

    let resp = client
        .get(format!("/get/get-rows?index=0&timestamp={ts}"))
        .cookie(crate::tests::bootstrap::cookie("bob", false))
        .header(Header::new(
            "Authorization",
            format!("Bearer {alice_token}"),
        ))
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Unauthorized,
        "bob must not reuse alice's snapshot on get-rows"
    );

    let resp = client
        .get(format!("/get/get-scroll-bar?timestamp={ts}"))
        .cookie(crate::tests::bootstrap::cookie("bob", false))
        .header(Header::new(
            "Authorization",
            format!("Bearer {alice_token}"),
        ))
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Unauthorized,
        "bob must not reuse alice's snapshot on get-scroll-bar"
    );
}

#[test]
fn b2_own_reuse_is_200() {
    let _g = lock();
    let client = setup_image_home();
    let (_album, _asset) = create_album(&client, "/b2_own/album/photo.jpg");
    crate::tests::bootstrap::seed("alice", false, None);

    let resp = client
        .post("/get/prefetch")
        .cookie(crate::tests::bootstrap::cookie("alice", false))
        .header(ContentType::JSON)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "alice prefetch");
    let body: Value = resp.into_json().expect("prefetch json");
    let ts = body["prefetch"]["timestamp"].as_i64().expect("timestamp");
    let alice_token = body["token"].as_str().expect("token").to_string();
    let alice_cookie = crate::tests::bootstrap::cookie("alice", false);

    let resp = client
        .get(format!("/get/get-data?timestamp={ts}&start=0&end=1"))
        .cookie(alice_cookie.clone())
        .header(Header::new(
            "Authorization",
            format!("Bearer {alice_token}"),
        ))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "own reuse get-data");

    let resp = client
        .get(format!("/get/get-rows?index=0&timestamp={ts}"))
        .cookie(alice_cookie.clone())
        .header(Header::new(
            "Authorization",
            format!("Bearer {alice_token}"),
        ))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "own reuse get-rows");

    let resp = client
        .get(format!("/get/get-scroll-bar?timestamp={ts}"))
        .cookie(alice_cookie)
        .header(Header::new(
            "Authorization",
            format!("Bearer {alice_token}"),
        ))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "own reuse get-scroll-bar");
}

#[test]
fn b2_share_holder_prefetch_roundtrip_still_200() {
    let _g = lock();
    let client = setup_image_home();
    let (album, asset) = create_album(&client, "/b2_share/album/photo.jpg");
    let share = create_share(&client, &album, true, true);
    let (ts, token, _) = prefetch_as_share(&client, &album, &share, Some(&asset));

    // get-data
    let resp = client
        .get(format!("/get/get-data?timestamp={ts}&start=0&end=1"))
        .header(Header::new("x-album-id", album.clone()))
        .header(Header::new("x-share-id", share.clone()))
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "share get-data");

    // get-rows
    let resp = client
        .get(format!("/get/get-rows?index=0&timestamp={ts}"))
        .header(Header::new("x-album-id", album.clone()))
        .header(Header::new("x-share-id", share.clone()))
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "share get-rows");

    // get-scroll-bar
    let resp = client
        .get(format!("/get/get-scroll-bar?timestamp={ts}"))
        .header(Header::new("x-album-id", album.clone()))
        .header(Header::new("x-share-id", share.clone()))
        .header(Header::new("Authorization", format!("Bearer {token}")))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "share get-scroll-bar");
}

#[test]
fn b2_admin_cannot_renew_another_users_snapshot() {
    let _g = lock();
    let client = setup_image_home();
    crate::tests::bootstrap::seed("bob", false, None);

    // bob prefetches a snapshot token (caller = "bob").
    let resp = client
        .post("/get/prefetch")
        .cookie(crate::tests::bootstrap::cookie("bob", false))
        .header(ContentType::JSON)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "bob prefetch");
    let body: Value = resp.into_json().expect("prefetch json");
    let bob_token = body["token"].as_str().expect("token").to_string();

    // admin attempts to renew bob's snapshot -> 401.
    let resp = client
        .post("/post/renew-timestamp-token")
        .cookie(auth_cookie(&client))
        .header(ContentType::JSON)
        .body(json!({ "token": bob_token }).to_string())
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Unauthorized,
        "admin must not renew another user's snapshot"
    );

    // admin renews own snapshot -> 200.
    let resp = client
        .post("/get/prefetch")
        .cookie(auth_cookie(&client))
        .header(ContentType::JSON)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "admin prefetch");
    let body: Value = resp.into_json().expect("prefetch json");
    let admin_token = body["token"].as_str().expect("token").to_string();

    let resp = client
        .post("/post/renew-timestamp-token")
        .cookie(auth_cookie(&client))
        .header(ContentType::JSON)
        .body(json!({ "token": admin_token }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "admin must renew own snapshot");
}

// ── B4: renew-hash-token is bound to the presenter's asset scope ────────

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

fn alice_personal_asset() -> (arrayvec::ArrayString<64>, arrayvec::ArrayString<64>) {
    // Returns (asset_id, content_hash) for alice's personal photo.
    let assets = crate::storage::asset_store::get_all_assets().expect("assets");
    let rec = assets
        .iter()
        .find(|r| r.path.ends_with("photo.jpg") && r.path.contains("users/alice"))
        .expect("alice personal asset");
    let asset_id = rec.asset_id;
    let hash = rec.content_hash.unwrap_or(asset_id);
    (asset_id, hash)
}

#[test]
fn b4_bob_cannot_renew_alices_personal_asset_token() {
    let _g = lock();
    let client = setup_image_home();
    let users_home = enable_users_home();
    seed("alice", false, None);
    seed("bob", false, None);

    let alice_dir = users_home.join("alice");
    std::fs::create_dir_all(&alice_dir).expect("alice dir");
    let photo = alice_dir.join("photo.jpg");
    snapfab::generate_batch(&[snapfab::PhotoSpec {
        output: Some(photo.to_string_lossy().into()),
        format: Some("jpeg".into()),
        width: Some(8),
        height: Some(8),
        tags: None,
        exif_date: None,
        minimal: false,
    }])
    .expect("gen");
    crate::process::dir_album::reset_dir_album_cache();
    crate::tasks::actor::album_index::index_album_with_root("/", false, None)
        .expect("index accepted");
    wait_for_index_completion();

    let (asset_id, hash) = alice_personal_asset();

    let ts: i64 = 999_999_999;
    let mut expired = ClaimsAsset::new(hash, asset_id, ts, false);
    expired.exp = 0;
    let expired_token = expired.encode();

    let bob_snapshot = ClaimsSnapshot::new("bob".to_string(), None, ts);

    let resp = client
        .post("/post/renew-hash-token")
        .header(Header::new(
            "Authorization",
            format!("Bearer {}", bob_snapshot.encode()),
        ))
        .header(ContentType::JSON)
        .body(json!({ "expiredHashToken": expired_token }).to_string())
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Unauthorized,
        "bob must not renew alice's personal asset token"
    );
}

#[test]
fn b4_alice_renews_own_expired_asset_token() {
    let _g = lock();
    let client = setup_image_home();
    let users_home = enable_users_home();
    seed("alice", false, None);

    let alice_dir = users_home.join("alice");
    std::fs::create_dir_all(&alice_dir).expect("alice dir");
    let photo = alice_dir.join("photo.jpg");
    snapfab::generate_batch(&[snapfab::PhotoSpec {
        output: Some(photo.to_string_lossy().into()),
        format: Some("jpeg".into()),
        width: Some(8),
        height: Some(8),
        tags: None,
        exif_date: None,
        minimal: false,
    }])
    .expect("gen");
    crate::process::dir_album::reset_dir_album_cache();
    crate::tasks::actor::album_index::index_album_with_root("/", false, None)
        .expect("index accepted");
    wait_for_index_completion();

    let (asset_id, hash) = alice_personal_asset();

    let ts: i64 = 999_999_998;
    let mut expired = ClaimsAsset::new(hash, asset_id, ts, false);
    expired.exp = 0;
    let expired_token = expired.encode();

    let alice_snapshot = ClaimsSnapshot::new("alice".to_string(), None, ts);

    let resp = client
        .post("/post/renew-hash-token")
        .header(Header::new(
            "Authorization",
            format!("Bearer {}", alice_snapshot.encode()),
        ))
        .header(ContentType::JSON)
        .body(json!({ "expiredHashToken": expired_token }).to_string())
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Ok,
        "alice must renew own expired asset token"
    );
    let body: Value = resp.into_json().expect("renew json");
    let renewed = body["token"].as_str().expect("token");
    let decoded: ClaimsAsset =
        decode_typed(renewed, &crate::router::auth::VALIDATION).expect("decode renewed");
    assert_eq!(decoded.hash, hash);
    assert_eq!(decoded.asset_id, asset_id);
    assert_eq!(decoded.timestamp, ts);
}

#[test]
fn b4_share_holder_renews_own_album_asset() {
    let _g = lock();
    let client = setup_image_home();
    let (album, asset) = create_album(&client, "/b4_share/album/photo.jpg");
    let share = create_share(&client, &album, true, true);
    let (ts, snapshot_token, _) = prefetch_as_share(&client, &album, &share, Some(&asset));

    let record = crate::storage::asset_store::get_asset_by_id(&asset)
        .expect("asset lookup")
        .expect("asset");
    let asset_id: arrayvec::ArrayString<64> = asset.parse().expect("asset id");
    let hash: arrayvec::ArrayString<64> = record
        .content_hash
        .unwrap_or(record.asset_id)
        .to_string()
        .parse()
        .expect("hash");

    let mut expired = ClaimsAsset::new(hash, asset_id, ts, false);
    expired.exp = 0;
    let expired_token = expired.encode();

    let resp = client
        .post("/post/renew-hash-token")
        .header(Header::new("x-album-id", album.clone()))
        .header(Header::new("x-share-id", share.clone()))
        .header(Header::new(
            "Authorization",
            format!("Bearer {snapshot_token}"),
        ))
        .header(ContentType::JSON)
        .body(json!({ "expiredHashToken": expired_token }).to_string())
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Ok,
        "share-holder must renew own-album asset token"
    );
    let body: Value = resp.into_json().expect("renew json");
    let renewed = body["token"].as_str().expect("token");
    let decoded: ClaimsAsset =
        decode_typed(renewed, &crate::router::auth::VALIDATION).expect("decode renewed");
    assert_eq!(decoded.hash, hash);
    assert_eq!(decoded.asset_id, asset_id);
    assert_eq!(decoded.timestamp, ts);
}
