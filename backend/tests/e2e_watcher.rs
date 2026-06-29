//! E2E test: `fs_notify_watcher` flag controls auto-indexing behaviour, and
//! `album_index` correctly scopes to the requested path.
//!
//! Sequence verified:
//! 1. `fs_notify_watcher = false` → placing a file does NOT auto-index it.
//! 2. `album_index("subdir_a")` → only the file in `subdir_a` is indexed.
//! 3. Enabling the watcher does NOT retroactively index pre-existing files.
//! 4. `album_index("/")` (root) → all un-indexed files are found.

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use picasu::{APP_CONFIG, AppConfig, build_rocket_with_config};
use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::Value;
use xtask::test_image::{PhotoSpec, generate_batch};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_client() -> Client {
    let config = APP_CONFIG.get().unwrap().read().unwrap().clone();
    Client::tracked(build_rocket_with_config(config)).expect("rocket client")
}

fn auth_cookie(client: &Client) -> rocket::http::Cookie<'static> {
    let r = client
        .post("/post/authenticate")
        .header(ContentType::JSON)
        .body(r#""""#)
        .dispatch();
    assert_eq!(r.status(), Status::Ok, "authenticate");
    let token = r.into_string().expect("token body");
    rocket::http::Cookie::new("jwt", token.trim_matches('"').to_owned())
}

/// Total number of items visible in the full library timeline.
fn indexed_count(client: &Client, image_home: &Path) -> u64 {
    let cookie = auth_cookie(client);
    let body = serde_json::json!({ "Path": image_home.to_string_lossy() });
    let r = client
        .post("/get/prefetch")
        .cookie(cookie)
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    assert_eq!(r.status(), Status::Ok, "prefetch");
    let v: Value = serde_json::from_slice(&r.into_bytes().expect("prefetch body")).unwrap();
    v["prefetch"]["dataLength"].as_u64().unwrap_or(0)
}

/// Trigger an album index on `album_path` (relative to IMAGE_HOME, `/` = root)
/// and block until the job reaches the `completed` state.
fn run_album_index(client: &Client, album_path: &str) {
    let cookie = auth_cookie(client);
    let body = serde_json::json!({ "album": album_path });
    let r = client
        .post("/post/index/album")
        .cookie(cookie.clone())
        .header(ContentType::JSON)
        .body(body.to_string())
        .dispatch();
    assert_eq!(r.status(), Status::Accepted, "album_index({album_path})");

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let r = client
            .get("/get/index/status")
            .cookie(cookie.clone())
            .dispatch();
        let v: Value = serde_json::from_slice(&r.into_bytes().expect("status body")).unwrap();
        match v["state"].as_str().unwrap_or("unknown") {
            "completed" => return,
            "failed" => panic!("album_index({album_path}) reported failure"),
            "canceled" => panic!("album_index({album_path}) was canceled"),
            _ => {}
        }
        assert!(
            Instant::now() < deadline,
            "album_index({album_path}) did not complete within 30 s"
        );
        thread::sleep(Duration::from_millis(100));
    }
}

/// Place a minimal JPEG at `image_home/relative` without triggering any scan.
fn place_photo(image_home: &Path, relative: &str) {
    let dest = image_home.join(relative);
    std::fs::create_dir_all(dest.parent().expect("parent dir")).expect("create dirs");
    generate_batch(&[PhotoSpec {
        output: Some(dest.to_string_lossy().to_string()),
        format: Some("jpeg".into()),
        width: Some(4),
        height: Some(4),
        tags: None,
        exif_date: None,
    }])
    .expect("generate test photo");
}

// ── Test ─────────────────────────────────────────────────────────────────────

#[test]
fn watcher_disabled_and_album_index_targeting() {
    // ── Setup ─────────────────────────────────────────────────────────────
    let dir = tempfile::tempdir().expect("tempdir");
    let data_dir = dir.path().join("data");
    let cfg_dir = dir.path().join("config");
    let image_home = dir.path().join("images");

    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::create_dir_all(&image_home).unwrap();

    // Start with watcher disabled so nothing is auto-indexed via FS events.
    std::fs::write(
        cfg_dir.join("config.toml"),
        "[gallery]\nfs_notify_watcher = false\n",
    )
    .unwrap();

    unsafe {
        std::env::set_var("PICASU_CONFIG_HOME", cfg_dir.to_str().unwrap());
        std::env::set_var("PICASU_DATA_HOME", data_dir.to_str().unwrap());
        std::env::set_var("PICASU_IMAGE_HOME", image_home.to_str().unwrap());
    }

    AppConfig::init();

    {
        let cfg = APP_CONFIG.get().unwrap().read().unwrap();
        assert!(!cfg.fs_notify_watcher, "watcher should start disabled");
    }

    // ── Phase 1: place file — nothing should be indexed ───────────────────
    place_photo(&image_home, "subdir_a/photo_a.jpg");

    let client = make_client();
    assert_eq!(
        indexed_count(&client, &image_home),
        0,
        "watcher disabled: file placed without scan must not appear in index"
    );

    // ── Phase 2: album_index on subdir_a — only photo_a indexed ──────────
    run_album_index(&client, "subdir_a");
    assert_eq!(
        indexed_count(&client, &image_home),
        1,
        "album_index(subdir_a) must index exactly the one file in that folder"
    );

    // ── Phase 3: add photo_b in subdir_b — still only 1 item ─────────────
    place_photo(&image_home, "subdir_b/photo_b.jpg");
    assert_eq!(
        indexed_count(&client, &image_home),
        1,
        "new file placed while watcher is disabled must not appear automatically"
    );

    // ── Phase 4: enabling the watcher must not retroactively scan ─────────
    {
        let mut cfg = APP_CONFIG.get().unwrap().write().unwrap();
        cfg.fs_notify_watcher = true;
    }
    // Give any spurious background activity a moment to settle.
    thread::sleep(Duration::from_millis(300));
    assert_eq!(
        indexed_count(&client, &image_home),
        1,
        "enabling the watcher must not retroactively index pre-existing files"
    );

    // Reset to disabled for the final scan phase.
    {
        let mut cfg = APP_CONFIG.get().unwrap().write().unwrap();
        cfg.fs_notify_watcher = false;
    }

    // ── Phase 5: album_index on root — both photos indexed ────────────────
    run_album_index(&client, "/");
    assert_eq!(
        indexed_count(&client, &image_home),
        2,
        "album_index(/) must index all files under image_home"
    );

    // ── Cleanup ───────────────────────────────────────────────────────────
    unsafe {
        std::env::remove_var("PICASU_CONFIG_HOME");
        std::env::remove_var("PICASU_DATA_HOME");
        std::env::remove_var("PICASU_IMAGE_HOME");
    }
}
