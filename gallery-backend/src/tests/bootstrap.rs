use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, RwLock};

use tempfile::TempDir;

use crate::operations::utils::image_path::get_resolved_image_path;
use crate::public::constant::redb::DATA_TABLE;
use crate::public::constant::storage::DATA_PATH;
use crate::public::db::tree::TREE;
use crate::public::structure::config::{APP_CONFIG, AppConfig};
use crate::router::builder::build_rocket_with_config;
use rocket::local::blocking::Client;

/// Holds the tempdir alive for the entire test binary run.
pub struct TestEnv {
    pub _dir: TempDir,
}

pub static TEST_ENV: LazyLock<TestEnv> = LazyLock::new(|| {
    let dir = tempfile::tempdir().expect("create tempdir");

    DATA_PATH
        .set(dir.path().to_path_buf())
        .expect("DATA_PATH already set");

    let mut test_config = AppConfig::default();
    test_config.public.image_path = Some("".into());
    APP_CONFIG
        .set(RwLock::new(test_config))
        .expect("APP_CONFIG already set");

    {
        let txn = TREE.in_disk.begin_write().expect("begin write txn");
        txn.open_table(DATA_TABLE).expect("create DATA_TABLE");
        txn.commit().expect("commit");
    }

    TestEnv { _dir: dir }
});

/// The image home path used by the test environment.
/// Derived from the test config that TEST_ENV sets up.
pub fn test_image_home() -> PathBuf {
    let _ = &*TEST_ENV;
    get_resolved_image_path().expect("IMAGE_HOME must be configured in test config")
}

/// Serialize the fields from `updates` into the public config and write a
/// config.json to the test directory for audit.  Affects the next call to
/// `make_client()` (the guard reads `APP_CONFIG` at request time).
///
/// Supported top-level keys (under `public`): `read_only_mode`, `image_path`.
pub fn write_config(updates: &serde_json::Value) {
    let mut config = APP_CONFIG.get().unwrap().write().unwrap();
    if let Some(obj) = updates.as_object() {
        if let Some(val) = obj.get("read_only_mode").and_then(|v| v.as_bool()) {
            config.public.read_only_mode = val;
        }
    }
    // Write a copy to disk for documentation/debugging.
    let data_path = DATA_PATH.get().expect("DATA_PATH set");
    let config_path = data_path.join("config.json");
    if let Ok(json) = serde_json::to_string_pretty(&*config) {
        let _ = std::fs::write(&config_path, json);
    }
}

/// There is exactly one global album-index slot in `album_index.rs`.
/// All generated tests that call `POST /post/index/album` must hold this
/// lock for their entire body to get `202 Accepted` instead of `409 Conflict`.
pub static INDEX_SERIAL_GUARD: Mutex<()> = Mutex::new(());

/// `TREE_SNAPSHOT` keys snapshots by `Utc::now().timestamp_millis()`
/// (get_prefetch.rs). Two `prefetch` calls from different tests landing
/// in the same millisecond will silently overwrite each other's
/// snapshot. Tests that call `prefetch_locate` must hold this guard for
/// their whole body to avoid tripping over it while running in parallel.
pub static PREFETCH_SERIAL_GUARD: Mutex<()> = Mutex::new(());

/// Build a Rocket test client with the current APP_CONFIG.
pub fn make_client() -> Client {
    let _ = &*TEST_ENV;
    let config = APP_CONFIG.get().unwrap().read().unwrap().clone();
    Client::tracked(build_rocket_with_config(config)).expect("valid rocket instance")
}
