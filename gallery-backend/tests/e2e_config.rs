use std::sync::Mutex;

use rocket::http::Status;
use rocket::local::blocking::Client;
use serde_json::Value;

use urocissa::{DATA_PATH, APP_CONFIG, AppConfig, build_rocket_with_config};

/// Serialise tests that touch global state.
static E2E_LOCK: Mutex<()> = Mutex::new(());

fn config_json(client: &Client) -> Value {
    let res = client.get("/get/config").dispatch();
    assert_eq!(res.status(), Status::Ok);
    res.into_json().expect("valid JSON")
}

/// End-to-end test: defaults → config.toml → env vars → API.
///
/// Spins up a Rocket client in-process after loading config through the
/// real AppConfig::init() path, then verifies via GET /get/config.
#[test]
fn e2e_config_precedence() {
    let _lock = E2E_LOCK.lock().unwrap();

    let dir = tempfile::tempdir().unwrap();
    let cfg_dir = dir.path().join("config");
    let dat_dir = dir.path().join("data");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::create_dir_all(&dat_dir).unwrap();

    // ── config.toml says port=9999, readOnly=true, max=200MiB ─────────
    std::fs::write(
        cfg_dir.join("config.toml"),
        r#"
[server]
port = 9999
max_upload_size = "200MiB"

[gallery]
read_only_mode = true
upload_folder = "my_uploads"
"#,
    )
    .unwrap();

    // ── Env overrides port, readOnly, upload folder ──────────────────
    unsafe {
        std::env::set_var("UROCISSA_CONFIG_HOME", cfg_dir.to_str().unwrap());
        std::env::set_var("UROCISSA_DATA_HOME", dat_dir.to_str().unwrap());
        std::env::set_var("UROCISSA_PORT", "7777");
        std::env::set_var("UROCISSA_READ_ONLY_MODE", "false");
        std::env::set_var("UROCISSA_UPLOAD_FOLDER", "env_folder");
        std::env::set_var("UROCISSA_IMAGE_HOME", dat_dir.join("images").to_str().unwrap());
    }

    // ── Load config through the real init path ────────────────────────
    // DATA_PATH is set inside init() from the config's data_home.
    // Create the db dir so Rocket's tree doesn't panic if touched.
    std::fs::create_dir_all(dat_dir.join("db")).unwrap();

    AppConfig::init();

    // ── Verify in-memory struct ───────────────────────────────────────
    {
        let cfg = APP_CONFIG.get().unwrap().read().unwrap();

        // Env wins over config file
        assert_eq!(cfg.port, 7777, "UROCISSA_PORT should override");
        assert!(!cfg.read_only_mode, "UROCISSA_READ_ONLY_MODE should override");
        assert_eq!(cfg.upload_folder, "env_folder", "UROCISSA_UPLOAD_FOLDER overrides");

        // Config file overrides defaults (no env)
        assert_eq!(cfg.max_upload_size, "200MiB", "toml max_upload_size");
        assert_eq!(cfg.data_home.as_deref(), Some(dat_dir.as_path()), "data_home from env");

        // Default fills gap
        assert_eq!(cfg.address, "0.0.0.0", "default address");
        assert!(!cfg.disable_img, "default disable_img");
    }

    // ── Verify via API ────────────────────────────────────────────────
    {
        let config = APP_CONFIG.get().unwrap().read().unwrap().clone();
        let client = Client::tracked(build_rocket_with_config(config)).unwrap();

        let json = config_json(&client);
        assert_eq!(json["port"].as_u64(), Some(7777));
        assert_eq!(json["readOnlyMode"].as_bool(), Some(false));
        assert_eq!(json["uploadFolder"].as_str(), Some("env_folder"));
        assert_eq!(json["maxUploadSize"].as_str(), Some("200MiB"));
        assert_eq!(json["address"].as_str(), Some("0.0.0.0"));
        assert_eq!(json["disableImg"].as_bool(), Some(false));
    }

    // ── Cleanup ───────────────────────────────────────────────────────
    unsafe {
        std::env::remove_var("UROCISSA_CONFIG_HOME");
        std::env::remove_var("UROCISSA_DATA_HOME");
        std::env::remove_var("UROCISSA_IMAGE_HOME");
        std::env::remove_var("UROCISSA_PORT");
        std::env::remove_var("UROCISSA_READ_ONLY_MODE");
        std::env::remove_var("UROCISSA_UPLOAD_FOLDER");
    }
}
