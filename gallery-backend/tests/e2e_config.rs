use std::sync::Mutex;

use rocket::http::Status;
use rocket::local::blocking::Client;
use serde_json::Value;

use urocissa::{APP_CONFIG, AppConfig, build_rocket_with_config};

static E2E_LOCK: Mutex<()> = Mutex::new(());

fn auth_and_config(client: &Client) -> Value {
    // Config.toml sets password = "secret123"
    let auth_res = client
        .post("/post/authenticate")
        .body("\"secret123\"")
        .header(rocket::http::ContentType::JSON)
        .dispatch();
    assert_eq!(auth_res.status(), Status::Ok, "authenticate should succeed");
    let token: String = auth_res.into_json().expect("JWT token");

    // GuardShare requires the JWT in a cookie named "jwt".
    let cookie = rocket::http::Cookie::new("jwt", token);
    let res = client.get("/get/config").cookie(cookie).dispatch();
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

    // ── Setup temp directories ────────────────────────────────────────
    let dir = tempfile::tempdir().unwrap();
    let cfg_dir = dir.path().join("config");
    let dat_dir = dir.path().join("data");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::create_dir_all(&dat_dir).unwrap();

    // ── Write config.toml ─────────────────────────────────────────────
    // For each field type:
    //   server.*  ← address, port, max_upload_size
    //   gallery.* ← data_home, image_home, upload_folder, read_only_mode, disable_img
    //   secrets.* ← password, auth_key
    //
    // Env overrides are set for port and upload_folder (verify env wins),
    // while max_upload_size and read_only_mode are left to the config file
    // (verify config wins over default).
    // address and disable_img are left out entirely (verify default).
    std::fs::write(
        cfg_dir.join("config.toml"),
        r#"
[server]
port = 9999
max_upload_size = "200MiB"

[gallery]
read_only_mode = true
upload_folder = "my_uploads"

[secrets]
password = "secret123"
auth_key = "jwt-key-from-toml"
"#,
    )
    .unwrap();

    // ── Set env vars ──────────────────────────────────────────────────
    unsafe {
        std::env::set_var("UROCISSA_CONFIG_HOME", cfg_dir.to_str().unwrap());
        std::env::set_var("UROCISSA_DATA_HOME", dat_dir.to_str().unwrap());
        std::env::set_var("UROCISSA_IMAGE_HOME", dat_dir.join("images").to_str().unwrap());

        // Override config file values
        std::env::set_var("UROCISSA_PORT", "7777");
        std::env::set_var("UROCISSA_READ_ONLY_MODE", "false");
        std::env::set_var("UROCISSA_UPLOAD_FOLDER", "env_folder");
        std::env::set_var("UROCISSA_MAX_UPLOAD_SIZE", "300MiB");

        // Set values not in config file
        std::env::set_var("UROCISSA_ADDRESS", "10.0.0.55");
        std::env::set_var("UROCISSA_DISABLE_IMG", "true");
        std::env::set_var("UROCISSA_AUTH_KEY", "jwt-key-from-env");
    }

    // ── Load config ───────────────────────────────────────────────────
    std::fs::create_dir_all(dat_dir.join("db")).unwrap();
    AppConfig::init();

    // ── Verify in-memory struct ───────────────────────────────────────
    {
        let cfg = APP_CONFIG.get().unwrap().read().unwrap();

        // ── Env wins over config file ─────────────
        assert_eq!(cfg.port, 7777, "UROCISSA_PORT overrides config.toml");
        assert!(!cfg.read_only_mode, "UROCISSA_READ_ONLY_MODE overrides");
        assert_eq!(cfg.upload_folder, "env_folder", "UROCISSA_UPLOAD_FOLDER overrides");
        assert_eq!(cfg.max_upload_size, "300MiB", "UROCISSA_MAX_UPLOAD_SIZE overrides");

        // ── Env sets values not in config ─────────
        assert_eq!(cfg.address, "10.0.0.55", "UROCISSA_ADDRESS");
        assert!(cfg.disable_img, "UROCISSA_DISABLE_IMG");

        // ── Env overrides secrets from toml ───────
        assert_eq!(
            cfg.auth_key.as_deref(),
            Some("jwt-key-from-env"),
            "UROCISSA_AUTH_KEY overrides secrets block"
        );


        // ── Config file wins over default ─────────
        assert_eq!(cfg.password.as_deref(), Some("secret123"), "password from secrets block");

        // ── DATA_HOME / IMAGE_HOME from env ───────
        assert_eq!(cfg.data_home.as_deref(), Some(dat_dir.as_path()), "UROCISSA_DATA_HOME");
        assert_eq!(
            cfg.image_home.as_deref(),
            Some(dat_dir.join("images").as_path()),
            "UROCISSA_IMAGE_HOME"
        );
    }

    // ── Verify via API ────────────────────────────────────────────────
    {
        let config = APP_CONFIG.get().unwrap().read().unwrap().clone();
        let client = Client::tracked(build_rocket_with_config(config)).unwrap();
        let json = auth_and_config(&client);

        assert_eq!(json["port"].as_u64(), Some(7777));
        assert_eq!(json["address"].as_str(), Some("10.0.0.55"));
        assert_eq!(json["readOnlyMode"].as_bool(), Some(false));
        assert_eq!(json["uploadFolder"].as_str(), Some("env_folder"));
        assert_eq!(json["maxUploadSize"].as_str(), Some("300MiB"));
        assert_eq!(json["disableImg"].as_bool(), Some(true));
    }

    // ── Cleanup ───────────────────────────────────────────────────────
    unsafe {
        std::env::remove_var("UROCISSA_CONFIG_HOME");
        std::env::remove_var("UROCISSA_DATA_HOME");
        std::env::remove_var("UROCISSA_IMAGE_HOME");
        std::env::remove_var("UROCISSA_PORT");
        std::env::remove_var("UROCISSA_ADDRESS");
        std::env::remove_var("UROCISSA_READ_ONLY_MODE");
        std::env::remove_var("UROCISSA_DISABLE_IMG");
        std::env::remove_var("UROCISSA_UPLOAD_FOLDER");
        std::env::remove_var("UROCISSA_MAX_UPLOAD_SIZE");
        std::env::remove_var("UROCISSA_AUTH_KEY");
    }
}
