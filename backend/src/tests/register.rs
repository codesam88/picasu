#![cfg(test)]

//! Step 5 registration matrix: one test per plan row, exercising the full
//! HTTP stack through a real Rocket client.
//!
//! Rows: open / invite / disabled / expired / double-use / revoked /
//! duplicate-id / short-password / legacy-deny, plus the invite-route auth
//! and no-token-leak requirements.
//!
//! Hashing budget: PBKDF2 rounds are slow in debug builds, so each test
//! seeds at most two users and batches many assertions per test. Invite
//! creation, cookie minting and listing go through paths that pay no KDF.

use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::{Value, json};

use crate::tests::bootstrap::{
    TEST_ENV, TEST_SERIAL_GUARD, make_client, reset_backend_state, write_config,
};

/// Serialize a test against the shared backend state.
fn lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_SERIAL_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Reset shared state and return a fresh client (knob off, no legacy
/// password, no users).
fn setup() -> Client {
    let _ = &*TEST_ENV;
    reset_backend_state();
    make_client()
}

/// Create a user record (no password hash — nothing to KDF) and mint the
/// matching admin cookie.
fn seed_admin(id: &str) -> rocket::http::Cookie<'static> {
    crate::auth::users::create_user(id, true).expect("seed admin");
    rocket::http::Cookie::new(
        "jwt",
        crate::router::auth::Claims::new_user(id.to_string(), true).encode(),
    )
}

/// Mint a cookie for an existing non-admin user (no hashing).
fn user_cookie(id: &str) -> rocket::http::Cookie<'static> {
    rocket::http::Cookie::new(
        "jwt",
        crate::router::auth::Claims::new_user(id.to_string(), false).encode(),
    )
}

fn set_legacy_password(password: Option<&str>) {
    let mut config = crate::model::config::APP_CONFIG
        .get()
        .expect("APP_CONFIG")
        .write()
        .expect("APP_CONFIG lock");
    config.password = password.map(str::to_string);
}

/// POST /post/users/register with the given body; returns the response.
fn register(client: &Client, body: &Value) -> (Status, Option<String>) {
    let resp = client
        .post("/post/users/register")
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    (resp.status(), resp.into_string())
}

/// POST /post/invites/create as `cookie` (or unauthenticated when `None`).
fn create_invite_route(
    client: &Client,
    cookie: Option<rocket::http::Cookie<'static>>,
) -> (Status, Option<Value>) {
    let mut req = client
        .post("/post/invites/create")
        .header(ContentType::JSON);
    if let Some(c) = cookie {
        req = req.cookie(c);
    }
    let resp = req.dispatch();
    let status = resp.status();
    let body = resp
        .into_string()
        .and_then(|b| serde_json::from_str(&b).ok());
    (status, body)
}

// ── Row: open ──────────────────────────────────────────────────────────────

/// With `allow_registration` on, anyone registers without an invite, gets
/// a non-admin account, and can log in with the password they set.
#[test]
fn open_registration_creates_non_admin_user() {
    let _g = lock();
    let client = setup();
    write_config(&json!({ "allow_registration": true }));

    let pw = crate::tests::fixtures::test_password("t-reg-open");
    let (status, body) = register(&client, &json!({ "userId": "s6-open", "password": pw }));
    assert_eq!(status, Status::Ok, "body: {body:?}");
    let created: Value = serde_json::from_str(&body.expect("body")).expect("user summary");
    assert_eq!(created["userId"], "s6-open");
    assert_eq!(created["admin"], false, "register is always non-admin");

    // The record exists as non-admin, and the password verifies.
    assert_eq!(
        crate::auth::users::get_user("s6-open").expect("get user"),
        Some(crate::auth::users::UserRecord { admin: false })
    );
    let resp = client
        .post("/post/authenticate")
        .header(ContentType::JSON)
        .body(json!({ "userId": "s6-open", "password": pw }).to_string())
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Ok,
        "registered user must be able to log in"
    );
}

// ── Row: invite ────────────────────────────────────────────────────────────

/// With the knob off, a valid invite registers the bearer and burns the
/// invite (marked used with the new user id).
#[test]
fn invite_registration_works_with_knob_off() {
    let _g = lock();
    let client = setup();
    assert!(
        !crate::model::config::APP_CONFIG
            .get()
            .expect("cfg")
            .read()
            .expect("lock")
            .allow_registration
    );

    let (token, _) = crate::auth::invites::create_invite("s6-root").expect("create invite");
    let pw = crate::tests::fixtures::test_password("t-reg-inv");
    let (status, body) = register(
        &client,
        &json!({ "userId": "s6-invited", "password": pw, "invite": token }),
    );
    assert_eq!(status, Status::Ok, "body: {body:?}");
    assert_eq!(
        crate::auth::users::get_user("s6-invited").expect("get user"),
        Some(crate::auth::users::UserRecord { admin: false })
    );
    let listed = crate::auth::invites::list_invites().expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].used_by.as_deref(), Some("s6-invited"));
}

// ── Row: disabled ──────────────────────────────────────────────────────────

/// Knob off and no (or empty) invite: registration is refused and no user
/// record appears.
#[test]
fn registration_without_invite_when_disabled_is_403() {
    let _g = lock();
    let client = setup();
    let pw = crate::tests::fixtures::test_password("t-reg-off");

    let (status, _) = register(&client, &json!({ "userId": "s6-noinv", "password": pw }));
    assert_eq!(status, Status::Forbidden);
    // An empty invite string counts as absent, not as a token.
    let (status, _) = register(
        &client,
        &json!({ "userId": "s6-noinv", "password": pw, "invite": "" }),
    );
    assert_eq!(status, Status::Forbidden);
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);
}

// ── Row: expired ───────────────────────────────────────────────────────────

/// An invite past its expiry does not register anybody.
#[test]
fn expired_invite_is_403() {
    let _g = lock();
    let client = setup();
    let past = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs()
        - 60;
    let (token, _) =
        crate::auth::invites::create_invite_with_expiry("s6-root", past).expect("create invite");
    let pw = crate::tests::fixtures::test_password("t-reg-exp");
    let (status, _) = register(
        &client,
        &json!({ "userId": "s6-late", "password": pw, "invite": token }),
    );
    assert_eq!(status, Status::Forbidden);
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);
}

// ── Row: double-use ────────────────────────────────────────────────────────

/// The second registration with the same invite is a conflict; the first
/// user survives and the invite stays bound to them.
#[test]
fn double_use_invite_is_409() {
    let _g = lock();
    let client = setup();
    let (token, _) = crate::auth::invites::create_invite("s6-root").expect("create invite");
    let pw = crate::tests::fixtures::test_password("t-reg-1");

    let (status, _) = register(
        &client,
        &json!({ "userId": "s6-first", "password": pw, "invite": token }),
    );
    assert_eq!(status, Status::Ok);
    let (status, _) = register(
        &client,
        &json!({ "userId": "s6-second", "password": pw, "invite": token }),
    );
    assert_eq!(status, Status::Conflict);
    assert_eq!(crate::auth::users::user_count().expect("count"), 1);
    assert!(
        crate::auth::users::get_user("s6-second")
            .expect("get")
            .is_none()
    );
    let listed = crate::auth::invites::list_invites().expect("list");
    assert_eq!(listed[0].used_by.as_deref(), Some("s6-first"));
}

// ── Row: revoked ───────────────────────────────────────────────────────────

/// Revoking removes the row, so a later redeem finds nothing (404).
#[test]
fn revoked_invite_is_404() {
    let _g = lock();
    let client = setup();
    let admin = seed_admin("s6-root");
    let (token, _) = crate::auth::invites::create_invite("s6-root").expect("create invite");
    let invite_id = crate::auth::invites::hash_token(&token);

    let resp = client
        .delete(format!("/delete/invites/{invite_id}"))
        .cookie(admin)
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    assert!(
        crate::auth::invites::list_invites()
            .expect("list")
            .is_empty()
    );

    let pw = crate::tests::fixtures::test_password("t-reg-rev");
    let (status, _) = register(
        &client,
        &json!({ "userId": "s6-revoked", "password": pw, "invite": token }),
    );
    assert_eq!(status, Status::NotFound);
    // Only the seeded admin exists — the revoked invite created nobody.
    assert_eq!(crate::auth::users::user_count().expect("count"), 1);
}

// ── Row: duplicate-id ──────────────────────────────────────────────────────

/// Registering an id that already exists is a conflict, not a takeover.
#[test]
fn duplicate_id_is_409() {
    let _g = lock();
    let client = setup();
    write_config(&json!({ "allow_registration": true }));
    let pw = crate::tests::fixtures::test_password("t-reg-dup");

    let (status, _) = register(&client, &json!({ "userId": "s6-dup", "password": pw }));
    assert_eq!(status, Status::Ok);
    let (status, _) = register(&client, &json!({ "userId": " s6-dup ", "password": pw }));
    assert_eq!(status, Status::Conflict, "trimmed id must collide");
    assert_eq!(crate::auth::users::user_count().expect("count"), 1);
    // The original record is untouched.
    assert_eq!(
        crate::auth::users::get_user("s6-dup").expect("get"),
        Some(crate::auth::users::UserRecord { admin: false })
    );
}

// ── Row: short-password ────────────────────────────────────────────────────

/// Floor-12 on registration: short, blank, and invalid ids are 400 and
/// leave no record behind.
#[test]
fn short_password_is_400() {
    let _g = lock();
    let client = setup();
    write_config(&json!({ "allow_registration": true }));
    let pw = crate::tests::fixtures::test_password("t-reg-400");

    let (status, _) = register(
        &client,
        &json!({ "userId": "s6-short", "password": "12345678901" }),
    );
    assert_eq!(status, Status::BadRequest);
    let (status, _) = register(&client, &json!({ "userId": "s6-short", "password": "   " }));
    assert_eq!(status, Status::BadRequest);
    let (status, _) = register(&client, &json!({ "userId": "   ", "password": pw }));
    assert_eq!(status, Status::BadRequest);
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);
}

// ── Row: legacy-deny (review A1) ───────────────────────────────────────────

/// With a legacy password configured, an unauthenticated `users/create` is
/// refused even while the store is empty — the host secret must gate
/// enrollment, not just login. (Migration stays possible via
/// `POST /post/authenticate`.)
#[test]
fn legacy_mode_denies_unauthenticated_create() {
    let _g = lock();
    let client = setup();
    set_legacy_password(Some(
        crate::tests::fixtures::test_password("t-reg-legacy").as_str(),
    ));

    let pw = crate::tests::fixtures::test_password("t-reg-a1");
    let resp = client
        .post("/post/users/create")
        .header(ContentType::JSON)
        .body(json!({ "userId": "s6-eve", "password": pw, "admin": false }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);

    set_legacy_password(None);
}

// ── Invite routes: auth + no token leak ────────────────────────────────────

/// Invite create/list/revoke are admin-only; the token appears exactly once
/// (create response) and never in listings.
#[test]
fn invite_routes_require_admin_and_never_leak_tokens() {
    let _g = lock();
    let client = setup();
    let admin = seed_admin("s6-root");
    crate::auth::users::create_user("s6-bob", false).expect("seed bob");

    // Unauthenticated and non-admin callers are refused on both routes.
    let (status, _) = create_invite_route(&client, None);
    assert_eq!(status, Status::Unauthorized);
    let (status, _) = create_invite_route(&client, Some(user_cookie("s6-bob")));
    assert_eq!(status, Status::Unauthorized);
    let resp = client.get("/get/invites").dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);

    // The admin's create returns the token exactly once…
    let (status, body) = create_invite_route(&client, Some(admin.clone()));
    assert_eq!(status, Status::Ok, "body: {body:?}");
    let body = body.expect("create body");
    let token = body["token"].as_str().expect("token");
    assert_eq!(token.len(), 22, "128-bit base64url token");
    assert_eq!(body["invite"]["createdBy"], "s6-root");

    // …and the listing carries metadata only — no token material.
    let resp = client.get("/get/invites").cookie(admin.clone()).dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let listed = resp.into_string().expect("list body");
    assert!(
        !listed.contains(token),
        "listing must never contain the token"
    );
    let parsed: Value = serde_json::from_str(&listed).expect("list json");
    assert_eq!(parsed.as_array().expect("array").len(), 1);
    assert!(parsed[0]["prefix"].is_string());
    assert!(parsed[0]["id"].is_string());

    // Revoking twice: second attempt is a 404 (already gone).
    let invite_id = parsed[0]["id"].as_str().expect("id").to_string();
    let resp = client
        .delete(format!("/delete/invites/{invite_id}"))
        .cookie(admin.clone())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let resp = client
        .delete(format!("/delete/invites/{invite_id}"))
        .cookie(admin)
        .dispatch();
    assert_eq!(resp.status(), Status::NotFound);
}
