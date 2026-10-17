#![cfg(test)]

//! Step 1 bootstrap-enrollment gate matrix: one test per plan row, exercising
//! the full HTTP stack through a real Rocket client.
//!
//! Rows: closed-by-default (flag off refuses `users/bootstrap`, `authenticate`
//! and `get/users`), open-with-flag on an empty store (probe, forced-admin
//! bootstrap, close-after-first-user, login), admin-less non-empty store stays
//! closed, no legacy gate, no `admin` field on any body, and the
//! boot-freshness refusal.
//!
//! The test harness opts the whole binary into bootstrap mode
//! (`tests/bootstrap.rs`); these tests opt back out with a flag-off config
//! where the row requires it.
//!
//! Hashing budget: PBKDF2 rounds are slow in debug builds, so each test seeds
//! at most two users and batches its assertions.

use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::json;

use crate::tests::bootstrap::{
    TEST_ENV, TEST_SERIAL_GUARD, make_client, reset_backend_state, write_config,
};
use crate::tests::fixtures::test_password;

/// Serialize a test against the shared backend state.
fn lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_SERIAL_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Reset shared state and return a fresh client. The harness resets into
/// bootstrap mode (flag on, empty store).
fn setup() -> Client {
    let _ = &*TEST_ENV;
    reset_backend_state();
    make_client()
}

fn set_bootstrap_admin(enabled: bool) {
    let mut config = crate::model::config::APP_CONFIG
        .get()
        .expect("APP_CONFIG")
        .write()
        .expect("APP_CONFIG lock");
    config.bootstrap_admin = enabled;
}

fn bootstrap_user(client: &Client, id: &str, password: &str) -> Status {
    client
        .post("/post/users/bootstrap")
        .header(ContentType::JSON)
        .body(json!({ "userId": id, "password": password }).to_string())
        .dispatch()
        .status()
}

fn login(client: &Client, id: &str, password: &str) -> Status {
    client
        .post("/post/authenticate")
        .header(ContentType::JSON)
        .body(json!({ "userId": id, "password": password }).to_string())
        .dispatch()
        .status()
}

// ── Row: closed by default ─────────────────────────────────────────────────

/// With the flag off (production default) and an empty store, every
/// enrollment and auth surface answers 401; no user is created.
#[test]
fn closed_by_default_denies_enrollment() {
    let _g = lock();
    let client = setup();
    write_config(&json!({ "bootstrap_admin": false }));
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);

    let resp = client
        .post("/post/users/bootstrap")
        .header(ContentType::JSON)
        .body(json!({ "userId": "root", "password": test_password("t-closed") }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);
    let body = resp.into_string().unwrap_or_default();
    assert!(
        body.to_lowercase().contains("no administrator enrolled"),
        "closed bootstrap must name the missing enrollment, got: {body}"
    );
    assert_eq!(
        login(&client, "root", &test_password("t-closed")),
        Status::Unauthorized
    );
    // The bare-string shape no longer parses as a login object: Rocket's
    // `Json` guard answers 422 for well-formed JSON of the wrong shape.
    let resp = client
        .post("/post/authenticate")
        .header(ContentType::JSON)
        .body(json!(test_password("t-closed")).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::UnprocessableEntity);
    // The open-mode identity is not minted, so the admin probe refuses.
    assert_eq!(
        client.get("/get/users").dispatch().status(),
        Status::Unauthorized
    );

    assert_eq!(crate::auth::users::user_count().expect("count"), 0);
}

// ── Row: open with the flag on an empty store ──────────────────────────────

/// With the flag on and an empty store, the open first-run behavior applies:
/// anonymous `GET /get/users` refuses (empty store has no admins; enrollment
/// state is served by `GET /get/server-info`), an unauthenticated bootstrap
/// mints the forced-admin first user, enrollment closes immediately after,
/// and the new password logs in.
#[test]
fn open_with_flag_on_empty_store_enrolls_first_admin() {
    let _g = lock();
    let client = setup();
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);

    // Tie-off: the admin-only user list refuses on an empty store.
    let resp = client.get("/get/users").dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);
    let body = resp.into_string().unwrap_or_default();
    assert!(
        body.to_lowercase().contains("no administrator enrolled"),
        "empty-store refusal must name the missing enrollment, got: {body}"
    );

    // Open-window enrollment is proven via server-info shape instead.
    let resp = client.get("/get/server-info").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let info: serde_json::Value =
        serde_json::from_str(&resp.into_string().expect("body")).expect("server-info body");
    assert_eq!(info["enrolled"], serde_json::Value::Bool(false));
    assert_eq!(info["enrollmentOpen"], serde_json::Value::Bool(true));

    // Unauthenticated bootstrap succeeds and forces admin:true. There is no
    // `admin` field on the body to ask otherwise.
    let pw = test_password("t-enroll");
    let resp = client
        .post("/post/users/bootstrap")
        .header(ContentType::JSON)
        .body(json!({ "userId": "root", "password": pw }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let created: serde_json::Value =
        serde_json::from_str(&resp.into_string().expect("body")).expect("created body");
    assert_eq!(created["admin"], true, "bootstrap must force admin:true");
    assert_eq!(
        crate::auth::users::get_user("root").expect("get user"),
        Some(crate::auth::users::UserRecord {
            admin: true,
            sessions: vec![]
        })
    );

    // The store is non-empty now, so enrollment is closed again.
    assert_eq!(
        bootstrap_user(&client, "second", &test_password("t-second")),
        Status::Unauthorized
    );

    // The enrolled admin can log in with the password it set.
    assert_eq!(login(&client, "root", &pw), Status::Ok);
}

// ── Row: first-create race ─────────────────────────────────────────────────

/// Concurrent unauthenticated bootstraps on an empty store must enroll
/// exactly one admin: the window check and the insert are atomic, so all
/// but one contender see the closed window (401) instead of landing
/// additional admins.
#[test]
fn concurrent_first_bootstraps_enroll_exactly_one_admin() {
    let _g = lock();
    let _client = setup();
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);

    const CONTENDERS: usize = 6;
    let barrier = std::sync::Barrier::new(CONTENDERS);
    let barrier_ref = &barrier;
    std::thread::scope(|s| {
        let mut handles = Vec::with_capacity(CONTENDERS);
        for i in 0..CONTENDERS {
            handles.push(s.spawn(move || {
                barrier_ref.wait();
                let client = make_client();
                bootstrap_user(
                    &client,
                    &format!("s6-racer-{i}"),
                    &test_password(&format!("t-race-{i}")),
                )
            }));
        }
        let statuses: Vec<Status> = handles
            .into_iter()
            .map(|h| h.join().expect("thread"))
            .collect();
        let wins = statuses.iter().filter(|&&s| s == Status::Ok).count();
        assert_eq!(wins, 1, "exactly one bootstrap must win, got: {statuses:?}");
        assert!(
            statuses
                .iter()
                .all(|s| *s == Status::Ok || *s == Status::Unauthorized),
            "losers must see the closed window (401), got: {statuses:?}"
        );
    });

    assert_eq!(crate::auth::users::user_count().expect("count"), 1);
    let users = crate::auth::users::list_users().expect("list users");
    assert_eq!(users.len(), 1);
    assert!(users[0].1.admin, "the single winner must be admin");
}

// ── Row: password-PUT never enrolls ─────────────────────────────────────────

/// A password PUT on an empty store must not enroll: the unknown target is
/// 404 even while the bootstrap window is open, and no user record or
/// password entry is written. Enrollment happens only via
/// `POST /post/users/bootstrap` (or the CLI); the first-run flow through
/// the settings password form is gone.
#[test]
fn password_put_on_empty_store_is_404_and_creates_nothing() {
    let _g = lock();
    let client = setup();
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);

    // No cookie: the open window admits the ephemeral admin identity, but
    // the canonical password route refuses the unknown target.
    let resp = client
        .put("/put/users/password")
        .header(ContentType::JSON)
        .body(json!({ "userId": "first", "newPassword": test_password("t-put-first") }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::NotFound);

    assert_eq!(
        crate::auth::users::get_user("first").expect("get user"),
        None
    );
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);
    let store = crate::auth::password::PasswdFile::load(&crate::auth::users::passwd_file_path())
        .expect("load passwd");
    assert!(store.is_empty(), "no password entry may be written");
}

// ── Row: admin-less non-empty store stays closed ───────────────────────────

/// The gate is "empty store", not "no admin": a non-admin user closes
/// enrollment even with the flag on (recovery is the CLI's job). No new user
/// may be created online.
#[test]
fn admin_less_non_empty_store_stays_closed() {
    let _g = lock();
    let client = setup();
    crate::auth::users::create_user("plain", false).expect("seed non-admin");

    assert_eq!(
        bootstrap_user(&client, "eve", &test_password("t-eve")),
        Status::Unauthorized
    );
    assert_eq!(crate::auth::users::user_count().expect("count"), 1);
}

// ── Row: no legacy password concept ──────────────────────────────────────

/// There is no host-secret gate anymore: flag-on opens the empty store and
/// flag-off closes it. A bare-string body never parses as a login object
/// (422) and never enrolls anybody.
#[test]
fn no_legacy_gate_flag_controls_empty_store() {
    let _g = lock();
    let client = setup();
    set_bootstrap_admin(true);
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);

    // Open without any password notion: the admin-only user list refuses,
    // and server-info reports the open window.
    let resp = client.get("/get/users").dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);
    let resp = client.get("/get/server-info").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let info: serde_json::Value =
        serde_json::from_str(&resp.into_string().expect("body")).expect("server-info body");
    assert_eq!(info["enrolled"], serde_json::Value::Bool(false));
    assert_eq!(info["enrollmentOpen"], serde_json::Value::Bool(true));
    // The bare-string shape is unparseable on an open store too (422).
    let resp = client
        .post("/post/authenticate")
        .header(ContentType::JSON)
        .body(json!(test_password("t-enroll-nolegacy")).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::UnprocessableEntity);
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);

    // Flag off closes the same empty store for object logins.
    set_bootstrap_admin(false);
    assert_eq!(
        login(&client, "root", &test_password("t-enroll-nolegacy")),
        Status::Unauthorized
    );
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);
    set_bootstrap_admin(true);
}

// ── Row: no `admin` field on any body ────────────────────────────────────

/// Creation never grants admin through a body flag: an unknown `admin` key
/// on the bootstrap body is a 4xx and enrolls nobody.
#[test]
fn bootstrap_rejects_unknown_admin_field() {
    let _g = lock();
    let client = setup();
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);

    let resp = client
        .post("/post/users/bootstrap")
        .header(ContentType::JSON)
        .body(
            json!({ "userId": "root", "password": test_password("t-enroll"), "admin": true })
                .to_string(),
        )
        .dispatch();
    assert_eq!(
        resp.status(),
        Status::UnprocessableEntity,
        "unknown `admin` field must be rejected, not honored"
    );
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);
}

// ── Boot freshness ─────────────────────────────────────────────────────────

/// The pure predicate: launching is refused only when the flag is set and a
/// password store already exists.
#[test]
fn bootstrap_freshness_predicate() {
    assert!(
        crate::bootstrap_freshness_ok(false, false),
        "flag off is always ok"
    );
    assert!(
        crate::bootstrap_freshness_ok(false, true),
        "flag off ignores the store"
    );
    assert!(
        crate::bootstrap_freshness_ok(true, false),
        "flag on a fresh store is ok"
    );
    assert!(
        !crate::bootstrap_freshness_ok(true, true),
        "flag on an existing store must refuse"
    );
}

/// The boot gate against a real `passwd.json` under the test DATA_PATH: flag
/// off launches, flag on an existing store refuses with an actionable message,
/// and removing the store re-opens.
#[test]
fn bootstrap_boot_gate_refuses_existing_passwd_file() {
    let _g = lock();
    let _ = &*TEST_ENV;
    reset_backend_state();

    let path = crate::auth::users::passwd_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("auth dir");
    }
    std::fs::write(&path, b"{\"users\":{}}").expect("write passwd");

    // Flag off: the store is not our business, launch.
    set_bootstrap_admin(false);
    assert!(crate::bootstrap_boot_gate().is_ok());

    // Flag on with an existing store: refuse, and name both remedies.
    set_bootstrap_admin(true);
    let err = crate::bootstrap_boot_gate().expect_err("must refuse to launch");
    let message = err.to_string();
    assert!(
        message.contains("bootstrap_admin"),
        "message names the flag: {message}"
    );
    assert!(
        message.contains("picasu-admin"),
        "message names the CLI remedy: {message}"
    );

    // Removing the store re-opens bootstrap.
    std::fs::remove_file(&path).expect("remove passwd");
    assert!(crate::bootstrap_boot_gate().is_ok());

    reset_backend_state();
}
