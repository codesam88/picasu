#![cfg(test)]

//! Step 5 registration matrix: one test per plan row, exercising the full
//! HTTP stack through a real Rocket client.
//!
//! Rows: open / invite / disabled / expired / double-use / revoked /
//! duplicate-id / short-password / closed-flag-off, plus the invite-route auth
//! and no-token-leak requirements.
//!
//! Hashing budget: PBKDF2 rounds are slow in debug builds, so each test
//! seeds at most two users and batches many assertions per test. Invite
//! creation, cookie minting and listing go through paths that pay no KDF.

use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::{Value, json};

use crate::tests::bootstrap::{cookie, lock, seed, setup, write_config};

/// POST /post/users/register with the given body; returns the response.
fn register(client: &Client, body: &Value) -> (Status, Option<String>) {
    let resp = client
        .post("/post/users/register")
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    (resp.status(), resp.into_string())
}

/// POST /post/invites as `cookie` (or unauthenticated when `None`)
/// with the given JSON body.
fn create_invite_route(
    client: &Client,
    cookie: Option<rocket::http::Cookie<'static>>,
    body: &Value,
) -> (Status, Option<Value>) {
    let mut req = client
        .post("/post/invites")
        .header(ContentType::JSON)
        .body(body.to_string());
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
    // Registration never creates the first user: the store is already
    // non-empty here, so the knob gates normally.
    seed("s6-root", true, None);
    let _admin = cookie("s6-root", true);

    let pw = crate::tests::fixtures::test_password("t-reg-open");
    let (status, body) = register(&client, &json!({ "userId": "s6-open", "password": pw }));
    assert_eq!(status, Status::Ok, "body: {body:?}");
    let created: Value = serde_json::from_str(&body.expect("body")).expect("user summary");
    assert_eq!(created["userId"], "s6-open");
    assert_eq!(created["admin"], false, "register is always non-admin");

    // The record exists as non-admin, and the password verifies.
    let record = crate::auth::users::user_record("s6-open")
        .expect("get user")
        .expect("the user must exist");
    assert_eq!(record.admin, false);
    assert!(record.hash.is_some(), "the password hash must be stored");
    let resp = client
        .post("/post/users/authenticate")
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
    // Registration never creates the first user: seed a non-admin so
    // the store is non-empty (invite registration on an admin-less store).
    crate::auth::users::create_user("s6-seed", false).expect("seed first user");
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
    let record = crate::auth::users::user_record("s6-invited")
        .expect("get user")
        .expect("the user must exist");
    assert_eq!(record.admin, false);
    assert!(record.hash.is_some(), "the password hash must be stored");
    let listed = crate::auth::invites::list_invites().expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].used_by.as_deref(), Some("s6-invited"));
    // The registered password must verify through the login route: the
    // entry hash lands inside `redeem_invite`, not in a later store write.
    let resp = client
        .post("/post/users/authenticate")
        .header(ContentType::JSON)
        .body(json!({ "userId": "s6-invited", "password": pw }).to_string())
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::Ok,
        "invited user must be able to log in with the registered password"
    );
}

// ── Row: disabled ──────────────────────────────────────────────────────────

/// Knob off and no (or empty) invite: registration is refused and no user
/// record appears.
#[test]
fn registration_without_invite_when_disabled_is_403() {
    let _g = lock();
    let client = setup();
    // Non-empty store: the 403 here pins the disabled gate, not the
    // empty-store 401 covered by the empty-store tests above.
    crate::auth::users::create_user("s6-seed", false).expect("seed first user");
    let pw = crate::tests::fixtures::test_password("t-reg-off");

    let (status, _) = register(&client, &json!({ "userId": "s6-noinv", "password": pw }));
    assert_eq!(status, Status::Forbidden);
    // An empty invite string counts as absent, not as a token.
    let (status, _) = register(
        &client,
        &json!({ "userId": "s6-noinv", "password": pw, "invite": "" }),
    );
    assert_eq!(status, Status::Forbidden);
    assert_eq!(crate::auth::users::user_count().expect("count"), 1);
}

// ── Row: empty store never registers ───────────────────────────────────────

/// Registration must not squat the first-user slot: on an empty store the
/// open knob still refuses with 401 naming bootstrap, and nobody is
/// created. `POST /post/users/bootstrap` is the only online first-user path.
#[test]
fn register_on_empty_store_is_401_even_with_knob_on() {
    let _g = lock();
    let client = setup();
    write_config(&json!({ "allow_registration": true }));

    let pw = crate::tests::fixtures::test_password("t-reg-r4-open");
    let (status, body) = register(&client, &json!({ "userId": "s6-r4-open", "password": pw }));
    assert_eq!(status, Status::Unauthorized, "body: {body:?}");
    let body = body.unwrap_or_default().to_lowercase();
    assert!(
        body.contains("no administrator enrolled"),
        "refusal must name the missing enrollment, got: {body}"
    );
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);
}

/// A valid invite does not bypass the empty-store refusal either: the first
/// user still comes from bootstrap, and the invite is not burned.
#[test]
fn register_on_empty_store_is_401_even_with_valid_invite() {
    let _g = lock();
    let client = setup();
    let (token, _) = crate::auth::invites::create_invite("s6-root").expect("create invite");

    let pw = crate::tests::fixtures::test_password("t-reg-r4-inv");
    let (status, body) = register(
        &client,
        &json!({ "userId": "s6-r4-invited", "password": pw, "invite": token }),
    );
    assert_eq!(status, Status::Unauthorized, "body: {body:?}");
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);
    assert!(
        crate::auth::users::user_record("s6-r4-invited")
            .expect("get")
            .is_none(),
        "refused registration must create nobody"
    );
    let listed = crate::auth::invites::list_invites().expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].used_by, None,
        "refused registration must not burn the invite"
    );
}

// ── Row: expired ───────────────────────────────────────────────────────────

/// An invite past its expiry does not register anybody.
#[test]
fn expired_invite_is_403() {
    let _g = lock();
    let client = setup();
    // Non-empty store so the 403 pins the expiry gate, not the empty-store
    // 401 covered by the empty-store tests above.
    crate::auth::users::create_user("s6-seed", false).expect("seed first user");
    let past = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs()
        - 60;
    let (token, _) = crate::auth::invites::create_invite_with_expiry("s6-root", Some(past), None)
        .expect("create invite");
    let pw = crate::tests::fixtures::test_password("t-reg-exp");
    let (status, _) = register(
        &client,
        &json!({ "userId": "s6-late", "password": pw, "invite": token }),
    );
    assert_eq!(status, Status::Forbidden);
    assert_eq!(crate::auth::users::user_count().expect("count"), 1);
}

// ── Row: double-use ────────────────────────────────────────────────────────

/// The second registration with the same invite is a conflict; the first
/// user survives and the invite stays bound to them.
#[test]
fn double_use_invite_is_409() {
    let _g = lock();
    let client = setup();
    // Non-empty store: the first registration below is not the first
    // user, so the 409 pins double-use rather than the empty-store refusal.
    crate::auth::users::create_user("s6-seed", false).expect("seed first user");
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
    // Seed + first user; the second registration created nobody.
    assert_eq!(crate::auth::users::user_count().expect("count"), 2);
    assert!(
        crate::auth::users::user_record("s6-second")
            .expect("get")
            .is_none()
    );
    let listed = crate::auth::invites::list_invites().expect("list");
    assert_eq!(listed[0].used_by.as_deref(), Some("s6-first"));
}

/// All registration-conflict surfaces answer an indistinguishable 409 body:
/// neither the response nor any echoed userId may reveal which gate failed
/// (the invite path is reachable without any session, so a distinguishable
/// body would enable enrollment probing).
#[test]
fn registration_conflict_bodies_are_indistinguishable() {
    let _g = lock();
    let client = setup();
    seed("s6-uni-root", true, None);
    seed("s6-uni-taken", false, None);
    let admin = cookie("s6-uni-root", true);
    let (invite_a, _) = crate::auth::invites::create_invite("s6-uni-root").expect("invite");
    let (invite_b, _) = crate::auth::invites::create_invite("s6-uni-root").expect("invite");
    let pw = crate::tests::fixtures::test_password("s6-uni");

    // (a) gated branch (admin session), target exists.
    let (status_a, body_a) = {
        let resp = client
            .post("/post/users/register")
            .header(ContentType::JSON)
            .cookie(admin)
            .body(json!({ "userId": "s6-uni-taken", "password": pw }).to_string())
            .dispatch();
        (resp.status(), resp.into_string())
    };
    // (c) invite path, target exists.
    let (status_c, body_c) = register(
        &client,
        &json!({ "userId": "s6-uni-taken", "password": pw, "invite": invite_a }),
    );
    // (b) invite path, invite already used: burn invite_b first, then reuse.
    let (status_ok, _) = register(
        &client,
        &json!({ "userId": "s6-uni-ok", "password": pw, "invite": invite_b }),
    );
    assert_eq!(status_ok, Status::Ok);
    let (status_b, body_b) = register(
        &client,
        &json!({ "userId": "s6-uni-again", "password": pw, "invite": invite_b }),
    );

    assert_eq!(status_a, Status::Conflict);
    assert_eq!(status_b, Status::Conflict);
    assert_eq!(status_c, Status::Conflict);
    assert_eq!(
        body_a, body_b,
        "gated vs invite-used bodies must be identical"
    );
    assert_eq!(
        body_a, body_c,
        "gated vs invite-user-taken bodies must be identical"
    );
    let echoed = body_a.as_deref().unwrap_or_default();
    assert!(
        !echoed.contains("s6-uni-taken") && !echoed.contains("s6-uni-ok"),
        "no userId may be echoed, got: {echoed}"
    );
}

// ── Row: revoked ───────────────────────────────────────────────────────────

/// Revoking removes the row, so a later redeem finds nothing (404).
#[test]
fn revoked_invite_is_404() {
    let _g = lock();
    let client = setup();
    seed("s6-root", true, None);
    let admin = cookie("s6-root", true);
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
    // Non-empty store so the first registration succeeds normally.
    crate::auth::users::create_user("s6-seed", false).expect("seed first user");
    let pw = crate::tests::fixtures::test_password("t-reg-dup");

    let (status, _) = register(&client, &json!({ "userId": "s6-dup", "password": pw }));
    assert_eq!(status, Status::Ok);
    let (status, _) = register(&client, &json!({ "userId": " s6-dup ", "password": pw }));
    assert_eq!(status, Status::Conflict, "trimmed id must collide");
    assert_eq!(crate::auth::users::user_count().expect("count"), 2);
    // The original record is untouched.
    let record = crate::auth::users::user_record("s6-dup")
        .expect("get")
        .expect("the user must exist");
    assert!(record.hash.is_some(), "the password hash must survive");
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
        &json!({ "userId": "s6-short", "password": "short1!" }),
    );
    assert_eq!(status, Status::BadRequest);
    let (status, _) = register(&client, &json!({ "userId": "s6-short", "password": "   " }));
    assert_eq!(status, Status::BadRequest);
    let (status, _) = register(&client, &json!({ "userId": "   ", "password": pw }));
    assert_eq!(status, Status::BadRequest);
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);
}

#[test]
fn trivial_passwords_flag_opens_registration_to_weak_passwords() {
    let _g = lock();
    let client = setup();
    write_config(&json!({ "allow_registration": true, "trivial_passwords": true }));
    // Non-empty store so the registration succeeds normally.
    crate::auth::users::create_user("s6-seed", false).expect("seed first user");
    let (status, _) = register(&client, &json!({ "userId": "s6-triv", "password": "pw" }));
    assert_eq!(status, Status::Ok);
}

// ── Row: admin session ───────────────────────────────────────────────────

/// A valid admin session registers without an invite; the account is still
/// non-admin.
#[test]
fn admin_session_creates_without_invite() {
    let _g = lock();
    let client = setup();
    seed("s6-root", true, None);
    let admin = cookie("s6-root", true);
    assert!(
        !crate::model::config::APP_CONFIG
            .get()
            .expect("cfg")
            .read()
            .expect("lock")
            .allow_registration,
        "knob must be off: the admin session alone must suffice"
    );

    let pw = crate::tests::fixtures::test_password("t-reg-admin");
    let resp = client
        .post("/post/users/register")
        .cookie(admin)
        .header(ContentType::JSON)
        .body(json!({ "userId": "s6-by-admin", "password": pw }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let created: Value = serde_json::from_str(&resp.into_string().expect("body")).expect("summary");
    assert_eq!(created["userId"], "s6-by-admin");
    assert_eq!(created["admin"], false, "register is always non-admin");
    let record = crate::auth::users::user_record("s6-by-admin")
        .expect("get user")
        .expect("the user must exist");
    assert_eq!(record.admin, false);
    assert!(record.hash.is_some(), "the password hash must be stored");
}

/// A non-admin session is anonymous for gating: without the knob or an
/// invite it is 403 and creates nobody.
#[test]
fn non_admin_session_without_invite_when_disabled_is_403() {
    let _g = lock();
    let client = setup();
    seed("s6-root", true, None);
    cookie("s6-root", true);
    crate::auth::users::create_user("s6-bob", false).expect("seed bob");
    let pw = crate::tests::fixtures::test_password("t-reg-bob");

    let (status, _) = {
        let resp = client
            .post("/post/users/register")
            .cookie(cookie("s6-bob", false))
            .header(ContentType::JSON)
            .body(json!({ "userId": "s6-bob-friend", "password": pw }).to_string())
            .dispatch();
        (resp.status(), resp.into_string())
    };
    assert_eq!(status, Status::Forbidden);
    assert!(
        crate::auth::users::user_record("s6-bob-friend")
            .expect("get")
            .is_none()
    );
    assert_eq!(crate::auth::users::user_count().expect("count"), 2);
}

// ── Row: no `admin` field on the body ──────────────────────────────────────

/// An unknown `admin` key on the register body is a 4xx and registers
/// nobody, even with the knob on.
#[test]
fn register_rejects_unknown_admin_field() {
    let _g = lock();
    let client = setup();
    write_config(&json!({ "allow_registration": true }));
    let pw = crate::tests::fixtures::test_password("t-reg-shape");

    let (status, _) = register(
        &client,
        &json!({ "userId": "s6-shape", "password": pw, "admin": false }),
    );
    assert_eq!(
        status,
        Status::UnprocessableEntity,
        "unknown `admin` field must be rejected, not honored"
    );
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);
}

// ── Invite routes: auth + no token leak ────────────────────────────────────

/// Invite create/list/revoke are admin-only; the token appears exactly once
/// (create response) and never in listings.
#[test]
fn invite_routes_require_admin_and_never_leak_tokens() {
    let _g = lock();
    let client = setup();
    seed("s6-root", true, None);
    let admin = cookie("s6-root", true);
    crate::auth::users::create_user("s6-bob", false).expect("seed bob");

    // Unauthenticated and non-admin callers are refused on both routes.
    let (status, _) = create_invite_route(&client, None, &json!({"expiresInHours": 168}));
    assert_eq!(status, Status::Unauthorized);
    let (status, _) = create_invite_route(
        &client,
        Some(cookie("s6-bob", false)),
        &json!({"expiresInHours": 168}),
    );
    assert_eq!(status, Status::Unauthorized);
    let resp = client.get("/get/invites").dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);

    // The admin's create returns the token exactly once…
    let (status, body) = create_invite_route(
        &client,
        Some(admin.clone()),
        &json!({"expiresInHours": 168}),
    );
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

/// `expiresInHours` configures the invite deadline; 0 means the invite
/// never expires, absent means the 7-day default. Negative values are
/// rejected before anything is stored.
#[test]
fn invite_expiry_is_configurable_in_hours() {
    let _g = lock();
    let client = setup();
    seed("s6-root", true, None);
    let admin = cookie("s6-root", true);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs();

    // 168 hours: the conventional 7-day lifetime.
    let (status, body) = create_invite_route(
        &client,
        Some(admin.clone()),
        &json!({"expiresInHours": 168}),
    );
    assert_eq!(status, Status::Ok, "body: {body:?}");
    let at = body.expect("body")["invite"]["expiresAt"]
        .as_u64()
        .expect("expiresAt number");
    assert!(
        at >= now + 6 * 24 * 3600 && at <= now + 7 * 24 * 3600 + 120,
        "168h is 7 days, got {at}"
    );

    // 1 hour: deadline ~1h out, and the token redeems.
    let (status, body) =
        create_invite_route(&client, Some(admin.clone()), &json!({"expiresInHours": 1}));
    assert_eq!(status, Status::Ok, "body: {body:?}");
    let body = body.expect("body");
    let at = body["invite"]["expiresAt"]
        .as_u64()
        .expect("expiresAt number");
    assert!(
        at >= now + 3500 && at <= now + 3600 + 120,
        "1h deadline, got {at}"
    );
    let token = body["token"].as_str().expect("token").to_string();
    let (status, _) = register(
        &client,
        &json!({"userId": "s6-hour", "password": "s6-hour-pw12", "invite": token}),
    );
    assert_eq!(status, Status::Ok);

    // 0: never expires (null deadline), redeems fine.
    let (status, body) =
        create_invite_route(&client, Some(admin.clone()), &json!({"expiresInHours": 0}));
    assert_eq!(status, Status::Ok, "body: {body:?}");
    let body = body.expect("body");
    assert!(
        body["invite"]["expiresAt"].is_null(),
        "0 means never expires"
    );
    let token = body["token"].as_str().expect("token").to_string();
    let (status, _) = register(
        &client,
        &json!({"userId": "s6-never", "password": "s6-never-pw12", "invite": token}),
    );
    assert_eq!(status, Status::Ok);

    // Negative: rejected, nothing stored.
    let (status, _) = create_invite_route(&client, Some(admin), &json!({"expiresInHours": -5}));
    assert_eq!(status, Status::BadRequest);
}

/// The create route lives at the noun path `/post/invites` (no verb in the
/// path); the operationId keeps the verb.
#[test]
fn invite_create_uses_noun_path() {
    let _g = lock();
    let client = setup();
    seed("s6-root", true, None);
    let admin = cookie("s6-root", true);
    let resp = client
        .post("/post/invites")
        .header(ContentType::JSON)
        .cookie(admin)
        .body(json!({"expiresInHours": 168}).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
}
