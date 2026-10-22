use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, RwLock};

use redb::ReadableTable;
use tempfile::TempDir;

use crate::model::config::{APP_CONFIG, AppConfig};
use crate::process::dir_album;
use crate::router::builder::build_rocket_with_config;
use crate::storage::cache::TREE_SNAPSHOT;
use crate::storage::db::METADATA_TABLE;
use crate::storage::db::TREE;
use crate::storage::files::DATA_PATH;
use crate::storage::files::get_resolved_image_home;
use rocket::local::blocking::Client;

/// Holds the tempdir alive for the entire test binary run.
pub struct TestEnv {
    pub _dir: TempDir,
}

pub static TEST_ENV: LazyLock<TestEnv> = LazyLock::new(|| {
    // Enable the test-only DB probe route for the entire `cargo test` process
    // (all scenario tests share one process).
    crate::router::get::get_test_probe::enable_test_probe();

    let dir = tempfile::tempdir().expect("create tempdir");
    let data_path = dir.path().to_path_buf();

    DATA_PATH
        .set(data_path.clone())
        .expect("DATA_PATH already set");

    let mut test_config = AppConfig::default();
    let image_home = data_path.join("images");
    std::fs::create_dir_all(&image_home).unwrap();
    test_config.image_home = Some(image_home);
    // Opt the whole test binary into bootstrap mode: scenario tests start
    // with the empty-store window open (unauthenticated first create via
    // `POST /post/users/bootstrap`). Production's default is closed, so the
    // closed-mode behavior needs its own explicit test (`tests/enroll_gate`).
    test_config.bootstrap_admin = true;
    APP_CONFIG
        .set(RwLock::new(test_config))
        .expect("APP_CONFIG already set");

    {
        let txn = TREE.in_disk.begin_write().expect("begin write txn");
        txn.open_table(METADATA_TABLE)
            .expect("create METADATA_TABLE");
        txn.open_table(crate::storage::db::ASSET_BY_PATH)
            .expect("create ASSET_BY_PATH");
        txn.open_table(crate::storage::db::ASSET_BY_ID)
            .expect("create ASSET_BY_ID");
        txn.open_table(crate::storage::db::DUPE_INDEX)
            .expect("create DUPE_INDEX");
        txn.open_table(crate::storage::db::REVOCATIONS)
            .expect("create REVOCATIONS");
        txn.commit().expect("commit");
    }

    TestEnv { _dir: dir }
});

/// The image home path used by the test environment.
/// Derived from the test config that TEST_ENV sets up.
pub fn test_image_home() -> PathBuf {
    let _ = &*TEST_ENV;
    get_resolved_image_home().expect("IMAGE_HOME must be configured in test config")
}

/// Serialize the fields from `updates` into the config and write a
/// config.toml to the test directory for audit.  Affects the next call to
/// `make_client()` (the guard reads `APP_CONFIG` at request time).
pub fn write_config(updates: &serde_json::Value) {
    let mut config = APP_CONFIG.get().unwrap().write().unwrap();
    if let Some(obj) = updates.as_object() {
        if let Some(val) = obj.get("read_only_mode").and_then(|v| v.as_bool()) {
            config.read_only_mode = val;
        }
        if let Some(val) = obj
            .get("allow_registration")
            .and_then(serde_json::Value::as_bool)
        {
            config.allow_registration = val;
        }
        if let Some(val) = obj
            .get("trivial_passwords")
            .and_then(serde_json::Value::as_bool)
        {
            config.trivial_passwords = val;
        }
        if let Some(val) = obj
            .get("bootstrap_admin")
            .and_then(serde_json::Value::as_bool)
        {
            config.bootstrap_admin = val;
        }
        if let Some(val) = obj.get("fs_notify_watcher").and_then(|v| v.as_bool()) {
            config.fs_notify_watcher = val;
        }
        if let Some(val) = obj.get("validate_upload_content").and_then(|v| v.as_bool()) {
            config.validate_upload_content = val;
        }
        if let Some(val) = obj
            .get("use_client_timestamp_info")
            .and_then(|v| v.as_bool())
        {
            config.use_client_timestamp_info = val;
        }
        if let Some(val) = obj
            .get("normalize_upload_filenames")
            .and_then(|v| v.as_bool())
        {
            config.normalize_upload_filenames = val;
        }
    }
    // Write a copy to disk for documentation/debugging.
    use serde::Serialize;
    #[derive(Serialize)]
    struct ConfigFile<'a> {
        picasu: &'a AppConfig,
    }
    let data_path = DATA_PATH.get().expect("DATA_PATH set");
    let config_path = data_path.join("config.toml");
    if let Ok(toml) = toml::to_string_pretty(&ConfigFile { picasu: &*config }) {
        let _ = std::fs::write(&config_path, toml);
    }
}

/// Serializes all scenario tests so they run one at a time.
/// The tests share a common DATA_PATH, TREE, and image directory.
/// Without this guard, parallel scenario tests collide on the indexer,
/// TREE_SNAPSHOT timestamp keys, APP_CONFIG mutations, and filesystem
/// state — causing non-deterministic failures.
pub static TEST_SERIAL_GUARD: Mutex<()> = Mutex::new(());

/// Reset all shared backend state between scenario tests.
/// This prevents data contamination across serialized scenarios
/// by clearing the database, caches, filesystem, and config mutations.
pub fn reset_backend_state() {
    // Clear in-memory caches.
    TREE_SNAPSHOT.in_memory.clear();
    TREE.in_memory.write().expect("TREE in_memory lock").clear();

    // Clear on-disk query snapshot cache tables.
    {
        let query_db = &crate::storage::cache::QUERY_SNAPSHOT.in_disk;
        if let Ok(txn) = query_db.begin_write() {
            // The query snapshot tables are named by version timestamp.
            // We can't enumerate table names in redb, but clearing the
            // in-memory cache above is sufficient for most cases.
            let _ = txn.commit();
        }
    }

    // Drain the on-disk index table.
    let txn = TREE
        .in_disk
        .begin_write()
        .expect("begin db write for cleanup");
    {
        let mut table = txn
            .open_table(METADATA_TABLE)
            .expect("open METADATA_TABLE for cleanup");
        let keys: Vec<String> = table
            .iter()
            .expect("iterate METADATA_TABLE")
            .map(|r| r.expect("read row").0.value().to_string())
            .collect();
        for key in &keys {
            table.remove(key.as_str()).expect("remove key");
        }
    }
    txn.commit().expect("commit METADATA_TABLE drain");

    // Also drain the path-primary asset tables and the user store.
    let txn = TREE
        .in_disk
        .begin_write()
        .expect("begin db write for asset table cleanup");
    for table_def in [
        crate::storage::db::ASSET_BY_PATH,
        crate::storage::db::ASSET_BY_ID,
        crate::storage::db::DUPE_INDEX,
        crate::storage::db::INVITES,
        crate::storage::db::REVOCATIONS,
    ] {
        let mut table = txn
            .open_table(table_def)
            .expect("open asset table for cleanup");
        let keys: Vec<String> = table
            .iter()
            .expect("iterate asset table")
            .filter_map(|r| r.ok().map(|(k, _)| k.value().to_string()))
            .collect();
        for key in &keys {
            table.remove(key.as_str()).expect("remove key");
        }
    }
    txn.commit().expect("commit asset table drain");

    // Wipe everything under DATA_PATH except the open `db/` directory.
    // `TREE_SNAPSHOT_IN_DISK` keeps `db/index_v5.redb` open for the whole
    // test binary, so its file must not be unlinked. This removes residual
    // content-addressed thumbnails (`object/compressed/...`), stale config
    // files, and any other on-disk residue a previous scenario left behind.
    let data_path = DATA_PATH.get().expect("DATA_PATH set");
    for entry in std::fs::read_dir(data_path)
        .unwrap_or_else(|e| panic!("read DATA_PATH {}: {e}", data_path.display()))
    {
        let entry = entry.expect("read DATA_PATH entry");
        if entry.file_name() == "db" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path)
                .unwrap_or_else(|e| panic!("remove {}: {e}", path.display()));
        } else {
            std::fs::remove_file(&path)
                .unwrap_or_else(|e| panic!("remove {}: {e}", path.display()));
        }
    }

    // Recreate the image directory.
    let image_home = test_image_home();
    std::fs::create_dir_all(&image_home)
        .unwrap_or_else(|e| panic!("create image_home {}: {e}", image_home.display()));

    // Remove the user store so authenticated scenarios never leak
    // password hashes into later (empty-store) scenarios. The DATA_PATH wipe
    // above already removes it in practice; this makes the guarantee
    // explicit and independent of wipe ordering.
    crate::auth::user_store::remove_store(&crate::auth::users::user_store_path());

    // Clear directory album cache so stale album IDs are not reused.
    dir_album::reset_dir_album_cache();

    // Reset config mutations (e.g. read_only_mode set by write_config).
    let mut config = APP_CONFIG
        .get()
        .expect("APP_CONFIG set")
        .write()
        .expect("APP_CONFIG lock");
    config.read_only_mode = false;
    config.allow_registration = false;
    config.trivial_passwords = false;
    // Tests run in bootstrap mode (empty-store window open); production's
    // default is closed. Individual tests opt out via `write_config`.
    config.bootstrap_admin = true;
    config.fs_notify_watcher = true;
    config.normalize_upload_filenames = true;
    config.validate_upload_content = true;
    config.use_client_timestamp_info = false;
    // Reset login-throttle knobs to the shipped defaults: throttle tests
    // shrink them and the tracker rebuilds from whatever is configured.
    // Canonical values live beside the schedules in `auth::rate_limit`.
    use crate::auth::rate_limit as throttle;
    config.trust_proxy_forwarded_for = false;
    config.login_ip_window_secs = throttle::LOGIN_IP_WINDOW_SECS;
    config.login_ip_max_failures = throttle::LOGIN_IP_MAX_FAILURES;
    config.login_ip_base_ban_secs = throttle::LOGIN_IP_BASE_BAN_SECS;
    config.login_ip_max_ban_secs = throttle::LOGIN_IP_MAX_BAN_SECS;
    config.login_ip_long_window_secs = throttle::LOGIN_IP_LONG_WINDOW_SECS;
    config.login_ip_long_max_failures = throttle::LOGIN_IP_LONG_MAX_FAILURES;
    config.login_ip_long_base_ban_secs = throttle::LOGIN_IP_LONG_BASE_BAN_SECS;
    config.login_ip_long_max_ban_secs = throttle::LOGIN_IP_LONG_MAX_BAN_SECS;
    config.login_user_long_window_secs = throttle::LOGIN_USER_LONG_WINDOW_SECS;
    config.login_user_long_max_failures = throttle::LOGIN_USER_LONG_MAX_FAILURES;
    config.login_user_long_base_ban_secs = throttle::LOGIN_USER_LONG_BASE_BAN_SECS;
    config.login_user_long_max_ban_secs = throttle::LOGIN_USER_LONG_MAX_BAN_SECS;

    // Drop the shared login throttle tracker so scenarios never inherit
    // failure counts or bans from a previous scenario.
    crate::router::post::users_authenticate::reset_login_throttle();
}

/// Build the test Rocket instance with the current APP_CONFIG, without a
/// client. Used by tests that inspect the mounted route table.
pub fn build_test_rocket() -> rocket::Rocket<rocket::Build> {
    let _ = &*TEST_ENV;
    let config = APP_CONFIG
        .get()
        .expect("APP_CONFIG set")
        .read()
        .expect("APP_CONFIG lock")
        .clone();
    build_rocket_with_config(config)
}

/// Build a Rocket test client with the current APP_CONFIG.
pub fn make_client() -> Client {
    Client::tracked(build_test_rocket()).expect("valid rocket instance")
}

/// Serialize a test against the shared backend state; every test module
/// funnels through here instead of module-local copies.
pub fn lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_SERIAL_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Reset shared state and return a fresh client (bootstrap mode, empty
/// store).
pub fn setup() -> Client {
    let _ = &*TEST_ENV;
    reset_backend_state();
    make_client()
}

/// [`setup`] plus `set_image_home`: required by discovery fixtures that
/// resolve paths through a module-level `OnceLock`, not the config.
pub fn setup_image_home() -> Client {
    let _ = &*TEST_ENV;
    reset_backend_state();
    crate::tests::fixtures::set_image_home(test_image_home());
    make_client()
}

/// Mint a `jwt` cookie for an existing user id without hashing (signs
/// locally). The token carries the user's current token version, so the
/// guard accepts it exactly like a freshly minted one.
pub fn cookie(id: &str, admin: bool) -> rocket::http::Cookie<'static> {
    let ver = crate::auth::users::user_record(id)
        .ok()
        .flatten()
        .map(|record| record.ver)
        .unwrap_or_default();
    let claims = crate::router::auth::Claims::new_user(id.to_string(), admin, ver);
    rocket::http::Cookie::new("jwt", claims.encode())
}

/// Create the user record and, when a password is given, write its hash
/// directly to the passwd file. Each given password pays one PBKDF2 round.
pub fn seed(id: &str, admin: bool, password: Option<&str>) {
    let generated;
    let pw = if let Some(pw) = password {
        pw
    } else {
        generated = crate::tests::fixtures::test_password(id);
        generated.as_str()
    };
    crate::auth::users::provision_user(id, pw, admin).expect("seed user");
}

/// Temporarily flip the bootstrap flag (enrollment-window tests opt out of
/// the harness default); `reset_backend_state` restores it.
pub fn set_bootstrap_admin(enabled: bool) {
    let mut config = crate::model::config::APP_CONFIG
        .get()
        .expect("APP_CONFIG set")
        .write()
        .expect("APP_CONFIG lock");
    config.bootstrap_admin = enabled;
}

/// Log in over HTTP; returns the status and, on success, the session
/// profile plus the raw token from the `Set-Cookie` header. Each call
/// pays one PBKDF2 round (two for unknown users: dummy verify).
pub fn login(
    client: &Client,
    user_id: &str,
    password: &str,
) -> (
    rocket::http::Status,
    Option<serde_json::Value>,
    Option<String>,
) {
    let resp = client
        .post("/post/users/authenticate")
        .header(rocket::http::ContentType::JSON)
        .body(serde_json::json!({ "userId": user_id, "password": password }).to_string())
        .dispatch();
    let status = resp.status();
    let ok = status == rocket::http::Status::Ok;
    let token = resp
        .headers()
        .get_one("Set-Cookie")
        .and_then(|h| h.split(';').next())
        .and_then(|pair| pair.strip_prefix("jwt="))
        .map(str::to_string)
        .filter(|_| ok);
    let profile: Option<serde_json::Value> = resp
        .into_string()
        .and_then(|b| serde_json::from_str(&b).ok())
        .filter(|_| ok);
    (status, profile, token)
}

/// [`login`] reduced to the status, for call sites that only assert codes.
pub fn login_status(client: &Client, user_id: &str, password: &str) -> rocket::http::Status {
    login(client, user_id, password).0
}

/// Decode a session cookie token back into claims (no hashing).
pub fn decode(token: &str) -> crate::router::auth::Claims {
    let validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    crate::router::auth::decode_typed(token, &validation).expect("decode token")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `reset_backend_state` must leave no on-disk residue between scenarios
    /// except the still-open redb database under `db/`. Otherwise a delete
    /// scenario can remove shared thumbnails in `object/compressed` that a
    /// later scenario still expects.
    #[test]
    fn reset_backend_state_wipes_data_path_residue() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        let data_path = DATA_PATH.get().expect("DATA_PATH set");

        let thumb = data_path.join("object/compressed/ab/abcdef0123456789.jpg");
        std::fs::create_dir_all(thumb.parent().unwrap()).unwrap();
        std::fs::write(&thumb, b"stale thumbnail").unwrap();
        let config_file = data_path.join("config.toml");
        std::fs::write(&config_file, b"stale config").unwrap();
        let db_file = data_path.join("db/index_v5.redb");
        assert!(db_file.exists(), "open redb file must exist before reset");

        reset_backend_state();

        assert!(!thumb.exists(), "residual thumbnail must be wiped");
        assert!(!config_file.exists(), "stale config.toml must be wiped");
        assert!(db_file.exists(), "open redb file must survive the wipe");
        assert!(test_image_home().exists(), "image_home must be recreated");
    }

    /// `reset_backend_state` must clear the user store (redb `SESSIONS` table)
    /// and remove the password file, so authenticated scenarios never leak
    /// users or password hashes into later (empty-store) scenarios.
    #[test]
    fn reset_backend_state_clears_user_store() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;

        // Establish entry state: sibling scenario tests leave user rows
        // behind (their setup cleans at start, not at end).
        reset_backend_state();

        crate::auth::users::create_user("s2b-reset-user", true).expect("create user");
        let passwd_path = crate::auth::users::user_store_path();
        assert_eq!(
            crate::auth::users::user_count().expect("user count before reset"),
            1
        );
        assert!(passwd_path.exists(), "passwd file must exist before reset");

        reset_backend_state();

        assert_eq!(
            crate::auth::users::user_count().expect("user count after reset"),
            0,
            "SESSIONS table must be empty after reset"
        );
        assert!(
            crate::auth::users::list_users()
                .expect("list users after reset")
                .is_empty()
        );
        assert_eq!(
            crate::auth::users::user_record("s2b-reset-user").expect("get user after reset"),
            None
        );
        assert!(
            !passwd_path.exists(),
            "passwd file must be removed by reset"
        );
    }
}
