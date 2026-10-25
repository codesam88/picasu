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
use serde_json::{Value, json};

use crate::router::auth::{Claims, Role};
use crate::tests::bootstrap::{cookie, decode, lock, login, seed, setup_image_home};
use crate::tests::fixtures::authz::{
    create_album, create_share, get_data_asset_token, get_data_rows, prefetch_as_share,
};
use crate::tests::fixtures::test_password;

fn role_of(claims: &Claims) -> (String, bool) {
    match &claims.role {
        Role::User { id, admin } => (id.clone(), *admin),
        Role::Share(_) => panic!("expected user role"),
    }
}

// ── Row 1: bootstrap ─────────────────────────────────────────────────────

/// The bootstrap window refuses logins and the first bootstrapped user is
/// forced admin.
///
/// Falsification companion: a second, unauthenticated bootstrap once the
/// store is non-empty must be 401 — otherwise bootstrap would stay open
/// forever and anyone could mint users.
#[test]
fn bootstrap_window_refuses_login_and_first_user_forced_admin() {
    let _g = lock();
    let client = setup_image_home();
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);

    // The window enrolls only via POST /post/users/bootstrap: a login with
    // any credentials takes the unknown-user path (401) and enrolls nobody.
    let (status, profile, token) = login(&client, "anything", &test_password("t-open"));
    assert_eq!(status, Status::Unauthorized);
    assert!(profile.is_none(), "refused login must carry no profile");
    assert!(token.is_none(), "refused login must set no cookie");
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);

    // First bootstrap forces `admin: true`: the body carries no `admin` key.
    let resp = client
        .post("/post/users/bootstrap")
        .header(ContentType::JSON)
        .body(json!({ "userId": "root", "password": test_password("t-root-pw") }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let created: Value = resp.into_json().expect("created user json");
    assert_eq!(created["admin"], true, "first user must be forced admin");
    let record = crate::auth::users::user_record("root")
        .expect("get user")
        .expect("the user must exist");
    assert!(record.admin);
    assert!(record.hash.is_some(), "the password hash must be stored");

    // Companion: bootstrap is one-shot — unauthenticated bootstrap now 401.
    let resp = client
        .post("/post/users/bootstrap")
        .header(ContentType::JSON)
        .body(json!({ "userId": "late", "password": test_password("t-late-pw") }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);

    // The enrolled admin logs in with the bootstrapped credentials.
    let (status, profile, token) = login(&client, "root", &test_password("t-root-pw"));
    assert_eq!(status, Status::Ok);
    let profile = profile.expect("root profile");
    assert_eq!(profile["userId"], "root");
    assert_eq!(profile["admin"], true);
    let (id, admin) = role_of(&decode(&token.expect("root token")));
    assert_eq!(id, "root");
    assert!(admin, "root login must mint an admin token");
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
    let client = setup_image_home();
    seed(
        "matrix-root",
        true,
        Some(test_password("t-root-pw").as_str()),
    );
    seed(
        "matrix-bob",
        false,
        Some(test_password("t-bob-pw").as_str()),
    );

    let (status, profile, token) = login(&client, "matrix-root", &test_password("t-root-pw"));
    assert_eq!(status, Status::Ok);
    let profile = profile.expect("root profile");
    assert_eq!(profile["userId"], "matrix-root");
    assert_eq!(profile["admin"], true);
    let (id, admin) = role_of(&decode(&token.expect("root token")));
    assert_eq!(id, "matrix-root");
    assert!(admin, "root login must mint an admin token");

    let (status, profile, token) = login(&client, "matrix-bob", &test_password("t-bob-pw"));
    assert_eq!(status, Status::Ok);
    let profile = profile.expect("bob profile");
    assert_eq!(profile["userId"], "matrix-bob");
    assert_eq!(profile["admin"], false);
    let (id, admin) = role_of(&decode(&token.expect("bob token")));
    assert_eq!(id, "matrix-bob");
    assert!(!admin, "bob login must mint a non-admin token");

    // Admin route: OK for root, 401 for bob.
    let resp = client
        .get("/get/users")
        .cookie(cookie("matrix-root", true))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let resp = client
        .get("/get/users")
        .cookie(cookie("matrix-bob", false))
        .dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);

    // Unknown user and wrong password are 401 (generic, no oracle).
    let (status, _, _) = login(&client, "matrix-ghost", &test_password("t-ghost"));
    assert_eq!(status, Status::Unauthorized);
    let (status, _, _) = login(&client, "matrix-bob", &test_password("t-wrong"));
    assert_eq!(status, Status::Unauthorized);
}

// ── Row 3: guest / share flow ────────────────────────────────────────────

/// Anonymous share prefetch + get-data + thumbnail serving work end-to-end
/// with no cookie; an admin-role user passes the share-guarded prefetch
/// without share headers; a non-admin user with share headers passes too.
///
/// The album + share are built first via the shared fixtures (which mint a
/// local admin session for a fixture record); users are seeded after,
/// since seeding does not invalidate shares. This keeps the fixture reuse
/// exact and the hashing budget at two seeds plus two logins.
///
/// Falsification companion: the same non-admin user *without* share headers
/// is 401 on the share-guarded route — otherwise the share guard would be
/// vacuous for any logged-in user.
#[test]
fn guest_share_prefetch_get_data_and_serving() {
    let _g = lock();
    let client = setup_image_home();

    // Album + share setup via the shared fixtures (local admin session).
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
    seed(
        "matrix-host",
        true,
        Some(test_password("t-host-pw").as_str()),
    );
    seed(
        "matrix-guest",
        false,
        Some(test_password("t-guest-pw").as_str()),
    );
    let (status, _, _) = login(&client, "matrix-host", &test_password("t-host-pw"));
    assert_eq!(status, Status::Ok, "host login must work");
    let (status, _, _) = login(&client, "matrix-guest", &test_password("t-guest-pw"));
    assert_eq!(status, Status::Ok, "guest login must work");

    // Admin-role user passes the share-guarded prefetch without share headers.
    let resp = client
        .post(format!("/get/prefetch?locate={asset_id}"))
        .cookie(cookie("matrix-host", true))
        .header(ContentType::JSON)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "admin without share headers");

    // Non-admin user with share headers passes too.
    let resp = client
        .post(format!("/get/prefetch?locate={asset_id}"))
        .cookie(cookie("matrix-guest", false))
        .header(Header::new("x-album-id", album_id.clone()))
        .header(Header::new("x-share-id", share_id.clone()))
        .header(ContentType::JSON)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok, "non-admin with share headers");

    // Companion: the same non-admin user without share headers is denied.
    let resp = client
        .post(format!("/get/prefetch?locate={asset_id}"))
        .cookie(cookie("matrix-guest", false))
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
    let client = setup_image_home();
    seed(
        "matrix-admin",
        true,
        Some(test_password("t-admin-old").as_str()),
    );
    seed(
        "matrix-member",
        false,
        Some(test_password("t-member-old").as_str()),
    );

    // Admin rotates the member's password without the old one.
    let resp = client
        .put("/put/users/password")
        .cookie(cookie("matrix-admin", true))
        .header(ContentType::JSON)
        .body(
            json!({ "userId": "matrix-member", "newPassword": test_password("t-member-new") })
                .to_string(),
        )
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);

    // Liveness: new password logs in, old password is 401.
    let (status, _, _) = login(&client, "matrix-member", &test_password("t-member-new"));
    assert_eq!(status, Status::Ok, "new password must work");
    let (status, _, _) = login(&client, "matrix-member", &test_password("t-member-old"));
    assert_eq!(
        status,
        Status::Unauthorized,
        "old password must stop working"
    );

    // A token minted while non-admin gains nothing on promotion on its own:
    // the token-embedded flag still gates — but a fresh login after
    // promotion mints an admin token, and that token loses access on
    // demotion without re-login (role re-read per request).
    let member_cookie = cookie("matrix-member", false);
    let resp = client
        .get("/get/users")
        .cookie(member_cookie.clone())
        .dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);

    crate::auth::users::set_admin("matrix-member", true).expect("promote member");
    let (status, _, token) = login(&client, "matrix-member", &test_password("t-member-new"));
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

// ── Row 5: no legacy migration ───────────────────────────────────────────
//
// There is no bare-string bootstrap path: `POST /post/users/authenticate` takes a
// `{ userId, password }` object only, and a JSON string body is 422 (covered
// by `router::post::users_authenticate::{bare_string_body_is_422_*}`). No migration
// registers users; enrollment is `POST /post/users/bootstrap` (open window)
// or the `picasu-cmd` CLI.

// ── Row 6: an offline-provisioned user ───────────────────────────────────

/// A user provisioned through the user store alone — the `picasu-cmd` CLI's
/// path — logs in and shows up in the admin listing. The store is the userdb
/// of record; the server keeps no second copy of the identity, so a user
/// that exists there is a user.
#[test]
fn offline_provisioned_user_can_log_in() {
    let _g = lock();
    let client = setup_image_home();
    // Provision exactly like the CLI does: the identity record only.
    crate::auth::user_store::provision(
        &crate::auth::users::user_store_path(),
        "matrix-offline",
        &test_password("t-offline-pw"),
        true,
        None,
    )
    .expect("provision offline");

    let (status, profile, _token) =
        login(&client, "matrix-offline", &test_password("t-offline-pw"));
    assert_eq!(
        status,
        Status::Ok,
        "an offline-provisioned user must be able to log in"
    );
    let profile = profile.expect("offline profile");
    assert_eq!(profile["userId"], "matrix-offline");
    assert_eq!(profile["admin"], true);

    // The admin listing reads the same store, so the user is in it.
    let resp = client
        .get("/get/users")
        .cookie(cookie("matrix-offline", true))
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let body: Value = resp.into_json().expect("user list");
    assert!(
        body.as_array()
            .expect("array")
            .iter()
            .any(|user| user["userId"] == "matrix-offline"),
        "the offline-provisioned user must be listed"
    );
}
