use std::sync::Mutex;

use rocket::http::{ContentType, Status};
use rocket::local::blocking::Client;
use serde_json::Value;

use picasu::{APP_CONFIG, AppConfig, build_rocket_with_config};

static API_LOCK: Mutex<()> = Mutex::new(());

fn auth_and_config(client: &Client) -> Value {
    // Seed the first admin via the open bootstrap window (empty store + flag
    // set by the test harness). Afterwards authenticate with the same JSON
    // body shape the SPA uses.
    let bootstrap_res = client
        .post("/post/users/bootstrap")
        .header(ContentType::JSON)
        .body(r#"{"userId":"admin","password":"secret123"}"#)
        .dispatch();
    assert_eq!(
        bootstrap_res.status(),
        Status::Ok,
        "bootstrap should succeed"
    );

    let auth_res = client
        .post("/post/authenticate")
        .header(ContentType::JSON)
        .body(r#"{"userId":"admin","password":"secret123"}"#)
        .dispatch();
    assert_eq!(auth_res.status(), Status::Ok, "authenticate should succeed");
    // Post-R5/R6 the body is a profile; the JWT travels in HttpOnly Set-Cookie.
    let set_cookie = auth_res
        .headers()
        .get_one("Set-Cookie")
        .expect("Set-Cookie");
    let pair = set_cookie.split(';').next().expect("cookie pair");
    let token = pair.strip_prefix("jwt=").expect("jwt cookie").to_owned();
    let cookie = rocket::http::Cookie::new("jwt", token);
    let res = client.get("/get/config").cookie(cookie).dispatch();
    assert_eq!(res.status(), Status::Ok);
    res.into_json().expect("valid JSON")
}

/// API test: defaults → config.toml → env vars → API.
///
/// Spins up a Rocket client in-process after loading config through the
/// real AppConfig::init() path, then verifies via GET /get/config.
#[test]
fn api_config_precedence() {
    let _lock = API_LOCK.lock().unwrap();

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
    //   secrets.* ← auth_key (password was removed with legacy auth)
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
auth_key = "jwt-key-from-toml"
"#,
    )
    .unwrap();

    // ── Set env vars ──────────────────────────────────────────────────
    unsafe {
        std::env::set_var("PICASU_CONFIG_HOME", cfg_dir.to_str().unwrap());
        std::env::set_var("PICASU_DATA_HOME", dat_dir.to_str().unwrap());
        std::env::set_var(
            "PICASU_IMAGE_HOME",
            dat_dir.join("images").to_str().unwrap(),
        );

        // Override config file values
        std::env::set_var("PICASU_PORT", "7777");
        std::env::set_var("PICASU_READ_ONLY_MODE", "false");
        std::env::set_var("PICASU_UPLOAD_FOLDER", "env_folder");
        std::env::set_var("PICASU_MAX_UPLOAD_SIZE", "300MiB");

        // Set values not in config file
        std::env::set_var("PICASU_ADDRESS", "10.0.0.55");
        std::env::set_var("PICASU_DISABLE_IMG", "true");
        std::env::set_var("PICASU_AUTH_KEY", "jwt-key-from-env");
    }

    // ── Load config ───────────────────────────────────────────────────
    std::fs::create_dir_all(dat_dir.join("db")).unwrap();
    AppConfig::init();
    // Open the first-run bootstrap window for this test binary (like
    // api_watcher and the shared bootstrap harness). Without it,
    // `POST /post/users/bootstrap` would 401 on a non-empty check and
    // `POST /post/authenticate` would require a pre-existing user.
    {
        let mut cfg = APP_CONFIG.get().unwrap().write().unwrap();
        cfg.bootstrap_admin = true;
    }

    // ── Verify in-memory struct ───────────────────────────────────────
    {
        let cfg = APP_CONFIG.get().unwrap().read().unwrap();

        // ── Env wins over config file ─────────────
        assert_eq!(cfg.port, 7777, "PICASU_PORT overrides config.toml");
        assert!(!cfg.read_only_mode, "PICASU_READ_ONLY_MODE overrides");
        assert_eq!(
            cfg.upload_folder, "env_folder",
            "PICASU_UPLOAD_FOLDER overrides"
        );
        assert_eq!(
            cfg.max_upload_size, "300MiB",
            "PICASU_MAX_UPLOAD_SIZE overrides"
        );

        // ── Env sets values not in config ─────────
        assert_eq!(cfg.address, "10.0.0.55", "PICASU_ADDRESS");
        assert!(cfg.disable_img, "PICASU_DISABLE_IMG");

        // ── Env overrides secrets from toml ───────
        assert_eq!(
            cfg.auth_key.as_deref(),
            Some("jwt-key-from-env"),
            "PICASU_AUTH_KEY overrides secrets block"
        );

        // ── DATA_HOME / IMAGE_HOME from env ───────
        assert_eq!(
            cfg.data_home.as_deref(),
            Some(dat_dir.as_path()),
            "PICASU_DATA_HOME"
        );
        assert_eq!(
            cfg.image_home.as_deref(),
            Some(dat_dir.join("images").as_path()),
            "PICASU_IMAGE_HOME"
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
        std::env::remove_var("PICASU_CONFIG_HOME");
        std::env::remove_var("PICASU_DATA_HOME");
        std::env::remove_var("PICASU_IMAGE_HOME");
        std::env::remove_var("PICASU_PORT");
        std::env::remove_var("PICASU_ADDRESS");
        std::env::remove_var("PICASU_READ_ONLY_MODE");
        std::env::remove_var("PICASU_DISABLE_IMG");
        std::env::remove_var("PICASU_UPLOAD_FOLDER");
        std::env::remove_var("PICASU_MAX_UPLOAD_SIZE");
        std::env::remove_var("PICASU_AUTH_KEY");
    }
}
