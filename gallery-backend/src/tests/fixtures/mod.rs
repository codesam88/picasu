use std::sync::{LazyLock, Mutex, RwLock};

use tempfile::TempDir;

use crate::public::constant::redb::DATA_TABLE;
use crate::public::constant::storage::DATA_PATH;
use crate::public::db::tree::TREE;
use crate::public::structure::config::APP_CONFIG;
use crate::public::structure::config::AppConfig;

pub use crate::operations::utils::image_path::get_resolved_image_path;
pub use crate::tasks::actor::folder_import::FolderImportState;

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

/// There is exactly one global folder-import slot in `folder_import.rs`.
/// All generated tests that call `POST /post/index` must hold this lock
/// for their entire body to get `202 Accepted` instead of `409 Conflict`.
pub static INDEX_SERIAL_GUARD: Mutex<()> = Mutex::new(());

/// `TREE_SNAPSHOT` keys snapshots by `Utc::now().timestamp_millis()`
/// (get_prefetch.rs). Two `prefetch` calls from different tests landing
/// in the same millisecond will silently overwrite each other's
/// snapshot, corrupting whichever test reads it second — a real
/// concurrency bug, not just a test artifact (see TODO.md). Tests that
/// call `prefetch_locate` must hold this guard for their whole body to
/// avoid tripping over it while running in parallel with each other.
pub static PREFETCH_SERIAL_GUARD: Mutex<()> = Mutex::new(());

mod api;
pub use api::*;
