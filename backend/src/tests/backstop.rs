use std::time::{Duration, Instant};

use rocket::local::blocking::Client;

use crate::model::config::APP_CONFIG;
use crate::storage::asset_store;
use crate::tasks::actor::album_index;
use crate::tasks::looper::start_backstop_loop;
use crate::tests::bootstrap::{lock, setup_image_home, test_image_home};
use snapfab::{PhotoSpec, generate_batch};

fn row_exists(path: &std::path::Path) -> bool {
    asset_store::get_asset_id_by_path(&path.to_string_lossy()).is_ok_and(|opt| opt.is_some())
}

fn gen_photo(path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create photo parent");
    }
    generate_batch(&[PhotoSpec {
        output: Some(path.to_string_lossy().into_owned()),
        format: Some("jpeg".into()),
        width: Some(4),
        height: Some(4),
        tags: None,
        exif_date: None,
        minimal: false,
    }])
    .expect("generate photo");
}

fn wait_for_index_idle() {
    let deadline = Instant::now() + Duration::from_secs(30);
    while album_index::album_index_status().state == album_index::AlbumIndexState::Running {
        assert!(Instant::now() <= deadline, "index did not finish");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn backstop_loop_indexes_missed_file() {
    let _guard = lock();
    let _client: Client = setup_image_home();
    wait_for_index_idle();

    // Configure a short (1s) backstop interval with the watcher disabled.
    {
        let mut cfg = APP_CONFIG.get().expect("APP_CONFIG").write().expect("lock");
        cfg.fs_notify_watcher = false;
        cfg.index_backstop_interval_secs = 1;
    }

    let image_home = test_image_home();
    let handle = start_backstop_loop().expect("backstop enabled");

    // Create a media file after the loop starts, without a watcher event.
    let photo = image_home.join("missed.jpg");
    gen_photo(&photo);

    let deadline = Instant::now() + Duration::from_millis(2500);
    while !row_exists(&photo) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }

    handle.abort();
    let appeared = row_exists(&photo);

    // Reset state.
    {
        let mut cfg = APP_CONFIG.get().expect("APP_CONFIG").write().expect("lock");
        cfg.index_backstop_interval_secs = 0;
        cfg.fs_notify_watcher = true;
    }
    wait_for_index_idle();

    assert!(
        appeared,
        "missed file should be indexed by the backstop loop"
    );
}

#[test]
fn backstop_loop_disabled_returns_none() {
    let _guard = lock();
    let _client: Client = setup_image_home();
    {
        let mut cfg = APP_CONFIG.get().expect("APP_CONFIG").write().expect("lock");
        cfg.index_backstop_interval_secs = 0;
    }
    assert!(start_backstop_loop().is_none());
}
