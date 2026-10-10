#![cfg(test)]

//! S5 integration matrix for multi-user auth: one test per plan row,
//! exercising the full HTTP stack through a real Rocket client.
//!
//! Hashing budget: PBKDF2 rounds are slow in debug builds, so each test
//! seeds at most two users and batches many assertions per test. Helpers
//! that need no hashing (cookie minting via [`Claims::encode`], token
//! decoding) stay off the hashing path; only user creation, password
//! writes, and login verifications pay a KDF round.

use rocket::http::{ContentType, Cookie, Header, Status};
use rocket::local::blocking::Client;
use serde_json::{Value, json};

use crate::router::auth::{Claims, Role};
use crate::tests::bootstrap::{
    TEST_ENV, TEST_SERIAL_GUARD, make_client, reset_backend_state, test_image_home,
};
use crate::tests::fixtures::authz::{
    create_album, create_share, get_data_asset_token, get_data_rows, prefetch_as_share,
};
use crate::tests::fixtures::set_image_home;

/// Serialize a test against the shared backend state.
fn lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_SERIAL_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Reset shared state and return a fresh client. `set_image_home` is required
/// because the discovery fixtures resolve paths through a module-level
/// `OnceLock`, not the config.
fn setup() -> Client {
    let _ = &*TEST_ENV;
    reset_backend_state();
    let data = test_image_home();
    set_image_home(data);
    make_client()
}

/// Create a user record plus its password hash directly in the stores.
/// Each call pays one PBKDF2 round; callers batch assertions afterwards.
fn seed_user(id: &str, admin: bool, password: &str) {
    crate::auth::users::create_user(id, admin).expect("seed user");
    let path = crate::auth::users::passwd_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("auth dir");
    }
    let mut store = crate::auth::password::PasswdFile::load(&path).expect("load passwd");
    store.set_password(id, password).expect("set password");
}

/// Mint a `jwt` cookie without hashing (signs locally, no store access).
fn user_cookie(id: &str, admin: bool) -> Cookie<'static> {
    Cookie::new("jwt", Claims::new_user(id.to_string(), admin).encode())
}

/// Log in over HTTP; returns the status and, on success, the raw token.
/// Each call pays one PBKDF2 round (two for unknown users: dummy verify).
fn login(client: &Client, user_id: &str, password: &str) -> (Status, Option<String>) {
    let resp = client
        .post("/post/authenticate")
        .header(ContentType::JSON)
        .body(json!({ "userId": user_id, "password": password }).to_string())
        .dispatch();
    let status = resp.status();
    let token = resp
        .into_string()
        .map(|b| b.trim_matches('"').to_string())
        .filter(|_| status == Status::Ok);
    (status, token)
}

/// Decode a login token body back into claims (no hashing).
fn decode(token: &str) -> Claims {
    let validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    crate::router::auth::decode_typed(token, &validation).expect("decode token")
}

fn role_of(claims: &Claims) -> (String, bool) {
    match &claims.role {
        Role::User { id, admin } => (id.clone(), *admin),
        Role::Share(_) => panic!("expected user role"),
    }
}

// ── Row 1: bootstrap ─────────────────────────────────────────────────────

/// Empty store is open (login with anything mints an admin token) and the
/// first created user is forced admin even when the body asks otherwise.
///
/// Falsification companion: a second, unauthenticated creation once the
/// store is non-empty must be 401 — otherwise bootstrap would stay open
/// forever and anyone could mint users.
#[test]
fn bootstrap_open_mode_and_first_user_forced_admin() {
    let _g = lock();
    let client = setup();
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);

    // Open first-run mode accepts any login body and mints an admin token.
    let (status, token) = login(&client, "anything", "whatever");
    assert_eq!(status, Status::Ok);
    let (id, admin) = role_of(&decode(&token.expect("open-mode token")));
    assert_eq!(id, "admin");
    assert!(admin, "open-mode identity must be admin");

    // First creation forces `admin: true` despite the body's `false`.
    let resp = client
        .post("/post/users/create")
        .header(ContentType::JSON)
        .body(json!({ "userId": "root", "password": "matrix-root-pw", "admin": false }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let created: Value = resp.into_json().expect("created user json");
    assert_eq!(created["admin"], true, "first user must be forced admin");
    assert_eq!(
        crate::auth::users::get_user("root").expect("get root"),
        Some(crate::auth::users::UserRecord { admin: true })
    );

    // Companion: bootstrap is one-shot — unauthenticated creation now 401.
    let resp = client
        .post("/post/users/create")
        .header(ContentType::JSON)
        .body(json!({ "userId": "late", "password": "matrix-late-pw", "admin": false }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);
}

// ── Row 2: two users ─────────────────────────────────────────────────────

/// Admin `root` + non-admin `bob`: each login mints a correctly-flagged
/// token; the admin route admits root and rejects bob (401); unknown users
/// and wrong passwords are 401.
///
/// The bob-is-401 assertion doubles as the falsification pin: any
/// authenticated user must not satisfy the admin guard.
#[test]
fn two_users_admin_route_distinguishes_roles() {
    let _g = lock();
    let client = setup();
    seed_user("matrix-root", true, "matrix-root-pw");
    seed_user("matrix-bob", false, "matrix-bob-pw");

    let (status, token) = login(&client, "matrix-root", "matrix-root-pw");
    assert_eq!(status, Status::Ok);
    let (id, admin) = role_of(&decode(&token.expect("root token")));
    assert_eq!(id, "matrix-root");
    assert!(admin, "root login must mint an admin token");

    let (status, token) = login(&client, "matrix-bob", "matrix-bob-pw");
    assert_eq!(status, Status::Ok);
    let (id, admin) = role_of(&decode(&token.expect("bob token")));
    assert_eq!(id, "matrix-bob");
    assert!(!admin, "bob login must mint a non-admin token");

    // Admin route: OK for root, 401 for bob.
    let resp = client
        .get("/get/users")
        .cookie(user_cookie("matrix-root", true))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let resp = client
        .get("/get/users")
        .cookie(user_cookie("matrix-bob", false))
        .dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);

    // Unknown user and wrong password are 401 (generic, no oracle).
    let (status, _) = login(&client, "matrix-ghost", "anything");
    assert_eq!(status, Status::Unauthorized);
    let (status, _) = login(&client, "matrix-bob", "wrong-password");
    assert_eq!(status, Status::Unauthorized);
}

// ── Row 3: guest / share flow ────────────────────────────────────────────

/// Anonymous share prefetch + get-data + thumbnail serving work end-to-end
/// with no cookie; an admin-role user passes the share-guarded prefetch
/// without share headers; a non-admin user with share headers passes too.
///
/// The album + share are built while the store is still empty (open mode,
/// so the shared fixtures' `auth_cookie` works); users are seeded after,
/// since seeding does not invalidate shares. This keeps the fixture reuse
/// exact and the hashing budget at two seeds plus two logins.
///
/// Falsification companion: the same non-admin user *without* share headers
/// is 401 on the share-guarded route — otherwise the share guard would be
/// vacuous for any logged-in user.
#[test]
fn guest_share_prefetch_get_data_and_serving() {
    let _g = lock();
    let client = setup();

    // Album + share setup in open mode, reusing the shared fixtures exactly.
    let (album_id, asset_id) = create_album(&client, "/matrix_guest/album/photo.jpg");
    let share_id = create_share(&client, &album_id, true, true, false);

    // Anonymous prefetch with share headers only (no cookie).
    let (ts, snapshot_token, _) = prefetch_as_share(&client, &album_id, &share_id, Some(&asset_id));

    // Anonymous get-data with the snapshot bearer token.
    let rows = get_data_rows(&client, ts, &snapshot_token, 1);
    assert!(!rows.is_empty(), "share get-data must return rows");

    // Anonymous thumbnail serving: the compressed file is keyed by content
    // hash, so read it out of the asset token's `hash` claim and serve that
    // exact path with share headers + the per-asset token.
    let asset_token = get_data_asset_token(&client, ts, &snapshot_token);
    let validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    let asset_claims: crate::router::auth::ClaimsHash =
        crate::router::auth::decode_typed(&asset_token, &validation).expect("decode asset token");
    let hash = asset_claims.hash.to_string();
    let resp = client
        .get(format!("/object/compressed/{}/{}.jpg", &hash[0..2], hash))
        .header(Header::new("x-album-id", album_id.clone()))
        .header(Header::new("x-share-id", share_id.clone()))
        .header(Header::new(
            "Authorization",
            format!("Bearer {asset_token}"),
        ))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "anonymous share serving");

    // Users afterwards: seeding does not invalidate the share above.
    seed_user("matrix-host", true, "matrix-host-pw");
    seed_user("matrix-guest", false, "matrix-guest-pw");
    let (status, _) = login(&client, "matrix-host", "matrix-host-pw");
    assert_eq!(status, Status::Ok, "host login must work");
    let (status, _) = login(&client, "matrix-guest", "matrix-guest-pw");
    assert_eq!(status, Status::Ok, "guest login must work");

    // Admin-role user passes the share-guarded prefetch without share headers.
    let resp = client
        .post(format!("/get/prefetch?locate={asset_id}"))
        .cookie(user_cookie("matrix-host", true))
        .header(ContentType::JSON)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "admin without share headers");

    // Non-admin user with share headers passes too.
    let resp = client
        .post(format!("/get/prefetch?locate={asset_id}"))
        .cookie(user_cookie("matrix-guest", false))
        .header(Header::new("x-album-id", album_id.clone()))
        .header(Header::new("x-share-id", share_id.clone()))
        .header(ContentType::JSON)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "non-admin with share headers");

    // Companion: the same non-admin user without share headers is denied.
    let resp = client
        .post(format!("/get/prefetch?locate={asset_id}"))
        .cookie(user_cookie("matrix-guest", false))
        .header(ContentType::JSON)
        .dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);
}

// ── Row 4: password change + demotion liveness ───────────────────────────

/// A password change takes effect on the next login (new password works,
/// old one 401s), and `set_admin` demotion/promotion is honored before
/// token expiry via per-request role re-read.
///
/// Falsification companion: the old password failing after the change pins
/// that the write really replaced the verifier (a no-op write that only
/// added a second hash would still accept the old password).
#[test]
fn password_change_and_demotion_take_effect() {
    let _g = lock();
    let client = setup();
    seed_user("matrix-admin", true, "matrix-admin-old");
    seed_user("matrix-member", false, "matrix-member-old");

    // Admin rotates the member's password without the old one.
    let resp = client
        .put("/put/users/password")
        .cookie(user_cookie("matrix-admin", true))
        .header(ContentType::JSON)
        .body(json!({ "userId": "matrix-member", "newPassword": "matrix-member-new" }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Liveness: new password logs in, old password is 401.
    let (status, _) = login(&client, "matrix-member", "matrix-member-new");
    assert_eq!(status, Status::Ok, "new password must work");
    let (status, _) = login(&client, "matrix-member", "matrix-member-old");
    assert_eq!(
        status,
        Status::Unauthorized,
        "old password must stop working"
    );

    // A token minted while non-admin gains nothing on promotion on its own:
    // the token-embedded flag still gates — but a fresh login after
    // promotion mints an admin token, and that token loses access on
    // demotion without re-login (role re-read per request).
    let member_cookie = user_cookie("matrix-member", false);
    let resp = client
        .get("/get/users")
        .cookie(member_cookie.clone())
        .dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);

    crate::auth::users::set_admin("matrix-member", true).expect("promote member");
    let (status, token) = login(&client, "matrix-member", "matrix-member-new");
    assert_eq!(status, Status::Ok, "login must work after promotion");
    let (_, admin) = role_of(&decode(&token.clone().expect("member token")));
    assert!(admin, "post-promotion login must mint an admin token");
    let promoted_cookie = Cookie::new("jwt", token.expect("member token"));
    let resp = client
        .get("/get/users")
        .cookie(promoted_cookie.clone())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "promotion must take effect");

    crate::auth::users::set_admin("matrix-member", false).expect("demote member");
    let resp = client.get("/get/users").cookie(promoted_cookie).dispatch();
    assert_eq!(
        resp.status(),
        Status::Unauthorized,
        "demotion must take effect before token expiry"
    );
}

// ── Row 5: legacy migration ──────────────────────────────────────────────
//
// Covered exactly by S3's `legacy_string_migrates_to_admin_user` in
// `router::post::authenticate` (sets `APP_CONFIG.password` directly, posts
// the legacy string body, asserts the `admin` user is created in both
// stores, the minted token passes an admin route, and a second legacy
// attempt is 401). Duplicating that flow here would add a PBKDF2-heavy
// round-trip with no new signal, so it is referenced, not repeated.
