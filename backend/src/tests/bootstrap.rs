use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, RwLock};

use redb::ReadableTable;
use tempfile::TempDir;

use crate::model::config::{APP_CONFIG, AppConfig};
use crate::router::builder::build_rocket_with_config;
use crate::storage::cache::TREE_SNAPSHOT;
use crate::storage::db::DATA_TABLE;
use crate::storage::db::TREE;
use crate::storage::files::DATA_PATH;
use crate::storage::files::get_resolved_image_home;
use rocket::local::blocking::Client;

/// Holds the tempdir alive for the entire test binary run.
pub struct TestEnv {
    pub _dir: TempDir,
}

pub static TEST_ENV: LazyLock<TestEnv> = LazyLock::new(|| {
    let dir = tempfile::tempdir().expect("create tempdir");
    let data_path = dir.path().to_path_buf();

    DATA_PATH
        .set(data_path.clone())
        .expect("DATA_PATH already set");

    let mut test_config = AppConfig::default();
    let image_home = data_path.join("images");
    std::fs::create_dir_all(&image_home).unwrap();
    test_config.image_home = Some(image_home);
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

    // Drain the on-disk index table.
    let txn = TREE
        .in_disk
        .begin_write()
        .expect("begin db write for cleanup");
    {
        let mut table = txn
            .open_table(DATA_TABLE)
            .expect("open DATA_TABLE for cleanup");
        let keys: Vec<String> = table
            .iter()
            .expect("iterate DATA_TABLE")
            .map(|r| r.expect("read row").0.value().to_string())
            .collect();
        for key in &keys {
            table.remove(key.as_str()).expect("remove key");
        }
    }
    txn.commit().expect("commit DATA_TABLE drain");

    // Wipe and recreate the image directory.
    let image_home = test_image_home();
    if image_home.exists() {
        std::fs::remove_dir_all(&image_home)
            .unwrap_or_else(|e| panic!("remove image_home {}: {e}", image_home.display()));
    }
    std::fs::create_dir_all(&image_home)
        .unwrap_or_else(|e| panic!("create image_home {}: {e}", image_home.display()));

    // Reset config mutations (e.g. read_only_mode set by write_config).
    let mut config = APP_CONFIG
        .get()
        .expect("APP_CONFIG set")
        .write()
        .expect("APP_CONFIG lock");
    config.read_only_mode = false;
}

/// Build a Rocket test client with the current APP_CONFIG.
pub fn make_client() -> Client {
    let _ = &*TEST_ENV;
    let config = APP_CONFIG.get().unwrap().read().unwrap().clone();
    Client::tracked(build_rocket_with_config(config)).expect("valid rocket instance")
}
