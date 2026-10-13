#![cfg(test)]

//! Step 1 bootstrap-enrollment gate matrix: one test per plan row, exercising
//! the full HTTP stack through a real Rocket client.
//!
//! Rows: closed-by-default (flag off refuses `users/create`, `authenticate`
//! and `get/users`), open-with-flag on an empty store (probe, forced-admin
//! create, close-after-first-user, login), admin-less non-empty store stays
//! closed, legacy mode ignores the flag, and the boot-freshness refusal.
//!
//! The test harness opts the whole binary into bootstrap mode
//! (`tests/bootstrap.rs`); these tests opt back out with a flag-off config or
//! a legacy password where the row requires it.
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
/// bootstrap mode (flag on, no legacy password, empty store).
fn setup() -> Client {
    let _ = &*TEST_ENV;
    reset_backend_state();
    make_client()
}

fn set_legacy_password(password: Option<&str>) {
    let mut config = crate::model::config::APP_CONFIG
        .get()
        .expect("APP_CONFIG")
        .write()
        .expect("APP_CONFIG lock");
    config.password = password.map(str::to_string);
}

fn set_bootstrap_admin(enabled: bool) {
    let mut config = crate::model::config::APP_CONFIG
        .get()
        .expect("APP_CONFIG")
        .write()
        .expect("APP_CONFIG lock");
    config.bootstrap_admin = enabled;
}

fn create_user(client: &Client, id: &str, password: &str) -> Status {
    client
        .post("/post/users/create")
        .header(ContentType::JSON)
        .body(json!({ "userId": id, "password": password, "admin": false }).to_string())
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

    assert_eq!(
        create_user(&client, "root", &test_password("t-closed")),
        Status::Unauthorized
    );
    assert_eq!(
        login(&client, "root", &test_password("t-closed")),
        Status::Unauthorized
    );
    // The legacy bare-string path is closed too (no migration target).
    let resp = client
        .post("/post/authenticate")
        .header(ContentType::JSON)
        .body(json!(test_password("t-closed")).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Unauthorized);
    // The open-mode identity is not minted, so the admin probe refuses.
    assert_eq!(
        client.get("/get/users").dispatch().status(),
        Status::Unauthorized
    );

    assert_eq!(crate::auth::users::user_count().expect("count"), 0);
}

// ── Row: open with the flag on an empty store ──────────────────────────────

/// With the flag on and an empty store, the open first-run behavior applies:
/// the probe succeeds with an empty list, an unauthenticated create mints the
/// forced-admin first user, enrollment closes immediately after, and the new
/// password logs in.
#[test]
fn open_with_flag_on_empty_store_enrolls_first_admin() {
    let _g = lock();
    let client = setup();
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);

    // Probe: the ephemeral admin identity lists an empty store.
    let resp = client.get("/get/users").dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let listed: Vec<serde_json::Value> =
        serde_json::from_str(&resp.into_string().expect("body")).expect("user list");
    assert!(listed.is_empty());

    // Unauthenticated create succeeds and forces admin:true.
    let pw = test_password("t-enroll");
    let resp = client
        .post("/post/users/create")
        .header(ContentType::JSON)
        .body(json!({ "userId": "root", "password": pw, "admin": false }).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    let created: serde_json::Value =
        serde_json::from_str(&resp.into_string().expect("body")).expect("created body");
    assert_eq!(created["admin"], true, "bootstrap must force admin:true");
    assert_eq!(
        crate::auth::users::get_user("root").expect("get user"),
        Some(crate::auth::users::UserRecord { admin: true })
    );

    // The store is non-empty now, so enrollment is closed again.
    assert_eq!(
        create_user(&client, "second", &test_password("t-second")),
        Status::Unauthorized
    );

    // The enrolled admin can log in with the password it set.
    assert_eq!(login(&client, "root", &pw), Status::Ok);
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
        create_user(&client, "eve", &test_password("t-eve")),
        Status::Unauthorized
    );
    assert_eq!(crate::auth::users::user_count().expect("count"), 1);
}

// ── Row: legacy mode ignores the flag ──────────────────────────────────────

/// A configured legacy password gates the store regardless of the flag: the
/// guards deny and unauthenticated create is refused, then a login with the
/// legacy password migrates the claimed id to first admin and works.
#[test]
fn legacy_mode_ignores_the_flag() {
    let _g = lock();
    let client = setup();
    let legacy = test_password("t-legacy-enroll");
    set_legacy_password(Some(&legacy));

    // The guard does not open despite the flag: the host secret wins.
    assert_eq!(
        client.get("/get/users").dispatch().status(),
        Status::Unauthorized
    );
    assert_eq!(
        create_user(&client, "eve", &test_password("t-eve")),
        Status::Unauthorized
    );
    assert_eq!(crate::auth::users::user_count().expect("count"), 0);

    // Migration via the legacy path registers the claimed id as the first
    // admin. The bare-string shape targets the historical `admin` id.
    let resp = client
        .post("/post/authenticate")
        .header(ContentType::JSON)
        .body(json!(legacy).to_string())
        .dispatch();
    assert_eq!(resp.status(), Status::Ok);
    assert_eq!(
        crate::auth::users::get_user("admin").expect("get user"),
        Some(crate::auth::users::UserRecord { admin: true })
    );
    set_legacy_password(None);
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
