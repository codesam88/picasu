use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use rocket::http::{ContentType, Header, Status};
use rocket::local::blocking::Client;
use serde_json::Value;
use snapfab::test_image::{PhotoSpec, generate_batch};

use picasu::{APP_CONFIG, AppConfig, build_rocket_with_config};

static API_LOCK: Mutex<()> = Mutex::new(());

/// Directories installed by [`ensure_config_init`]. Owned by a process-wide
/// `OnceLock` so the tempdirs (DB, user store, image home) outlive every
/// test in this binary: `AppConfig::init()` panics on a second call, so the
/// precedence env/config is installed exactly once and all tests in this
/// file share the live globals.
struct SharedSetup {
    _tmp: tempfile::TempDir,
    cfg_dir: PathBuf,
    dat_dir: PathBuf,
}

static SHARED_SETUP: OnceLock<SharedSetup> = OnceLock::new();

/// Install the precedence test's env + config.toml and run
/// `AppConfig::init()` exactly once per test binary. Later callers reuse
/// the live `APP_CONFIG` (see [`SharedSetup`]).
fn ensure_config_init() -> &'static SharedSetup {
    SHARED_SETUP.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let cfg_dir = dir.path().join("config");
        let dat_dir = dir.path().join("data");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::create_dir_all(&dat_dir).unwrap();

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

        // ── Load config ───────────────────────────────────────────────
        std::fs::create_dir_all(dat_dir.join("db")).unwrap();
        AppConfig::init();
        // Open the first-run bootstrap window for this test binary (like
        // api_watcher and the shared bootstrap harness). Without it,
        // `POST /post/users/bootstrap` would 401 on a non-empty check and
        // `POST /post/users/authenticate` would require a pre-existing user.
        {
            let mut cfg = APP_CONFIG.get().unwrap().write().unwrap();
            cfg.bootstrap_admin = true;
        }

        SharedSetup {
            _tmp: dir,
            cfg_dir,
            dat_dir,
        }
    })
}

fn auth_and_config(client: &Client) -> Value {
    // Seed the first admin via the open bootstrap window (empty store + flag
    // set by the test harness). Afterwards authenticate with the same JSON
    // body shape the SPA uses. The window is one-shot per binary: when a
    // sibling test already enrolled the admin, bootstrap answers 401 and the
    // login below reuses that account.
    let bootstrap_res = client
        .post("/post/users/bootstrap")
        .header(ContentType::JSON)
        .body(r#"{"userId":"admin","password":"secret123"}"#)
        .dispatch();
    assert!(
        bootstrap_res.status() == Status::Ok || bootstrap_res.status() == Status::Unauthorized,
        "bootstrap should succeed on an empty store or report a closed window, got {}",
        bootstrap_res.status()
    );

    let auth_res = client
        .post("/post/users/authenticate")
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
    let _lock = API_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    // ── Setup temp directories ────────────────────────────────────────
    let setup = ensure_config_init();
    let cfg_dir = setup.cfg_dir.clone();
    let dat_dir = setup.dat_dir.clone();

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

        // `address`/`port` are host fields, not client config: they are
        // asserted on the in-memory struct above and must not leak into the
        // `GET /get/config` body at any tier.
        assert!(json.get("address").is_none(), "address must be absent");
        assert!(json.get("port").is_none(), "port must be absent");
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

/// R13 (B+C): `GET /get/config` is `GuardShare`-gated, so share-tier
/// callers must not see host fields. `address`/`port` are gone for every
/// tier; `imagePath`/`uploadFolder` are `null` for share-credential
/// callers while the admin tier keeps the real values. A non-admin
/// session never reaches the handler at all: `GuardShare` only admits
/// admins and shares, so it is refused with 401.
#[test]
fn share_tier_config_hides_host_fields() {
    let _lock = API_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _setup = ensure_config_init();
    let config = APP_CONFIG.get().unwrap().read().unwrap().clone();
    // Untracked: every request carries exactly the credential under test —
    // no cookie jar could upgrade the share-tier calls to the admin tier.
    let client = Client::untracked(build_rocket_with_config(config)).unwrap();

    // ── Admin session (bootstrap only the empty store) ────────────────
    let admin_body = r#"{"userId":"admin","password":"secret123"}"#;
    let bootstrap = client
        .post("/post/users/bootstrap")
        .header(ContentType::JSON)
        .body(admin_body)
        .dispatch();
    assert!(
        bootstrap.status() == Status::Ok || bootstrap.status() == Status::Unauthorized,
        "bootstrap enrolls once, then reports a closed window, got {}",
        bootstrap.status()
    );
    let auth_res = client
        .post("/post/users/authenticate")
        .header(ContentType::JSON)
        .body(admin_body)
        .dispatch();
    assert_eq!(auth_res.status(), Status::Ok, "authenticate admin");
    let set_cookie = auth_res
        .headers()
        .get_one("Set-Cookie")
        .expect("Set-Cookie");
    let admin_token = set_cookie
        .split(';')
        .next()
        .expect("cookie pair")
        .strip_prefix("jwt=")
        .expect("jwt cookie")
        .to_owned();
    let admin = || rocket::http::Cookie::new("jwt", admin_token.clone());

    // ── Non-admin user: an admin session registers without an invite ──
    let reg = client
        .post("/post/users/register")
        .cookie(admin())
        .header(ContentType::JSON)
        .body(r#"{"userId":"viewer","password":"viewer-secret-9"}"#)
        .dispatch();
    assert_eq!(reg.status(), Status::Ok, "register viewer");
    let viewer_auth = client
        .post("/post/users/authenticate")
        .header(ContentType::JSON)
        .body(r#"{"userId":"viewer","password":"viewer-secret-9"}"#)
        .dispatch();
    assert_eq!(viewer_auth.status(), Status::Ok, "authenticate viewer");
    let viewer_token = viewer_auth
        .headers()
        .get_one("Set-Cookie")
        .expect("Set-Cookie")
        .split(';')
        .next()
        .expect("cookie pair")
        .strip_prefix("jwt=")
        .expect("jwt cookie")
        .to_owned();
    let viewer = || rocket::http::Cookie::new("jwt", viewer_token.clone());

    // ── One indexed photo, so a real share can exist ─────────────────
    let image_home = APP_CONFIG
        .get()
        .unwrap()
        .read()
        .unwrap()
        .image_home
        .clone()
        .expect("PICASU_IMAGE_HOME set by ensure_config_init");
    let photo = image_home.join("sharecfg/photo.jpg");
    std::fs::create_dir_all(photo.parent().expect("photo parent")).expect("create dirs");
    generate_batch(&[PhotoSpec {
        output: Some(photo.to_string_lossy().into_owned()),
        format: Some("jpeg".into()),
        width: Some(8),
        height: Some(8),
        tags: None,
        exif_date: None,
        minimal: false,
    }])
    .expect("generate photo");
    let index = client
        .post("/post/index/album")
        .cookie(admin())
        .header(ContentType::JSON)
        .body(r#"{"album": "/sharecfg"}"#)
        .dispatch();
    assert_eq!(index.status(), Status::Accepted, "album_index");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status = client.get("/get/index/status").cookie(admin()).dispatch();
        assert_eq!(status.status(), Status::Ok, "index status");
        let body: Value = status.into_json().expect("status json");
        match body["state"].as_str().unwrap_or("unknown") {
            "completed" => break,
            "failed" => panic!("album_index reported failure"),
            "canceled" => panic!("album_index was canceled"),
            _ => {}
        }
        assert!(
            Instant::now() < deadline,
            "album_index(/sharecfg) did not complete within 30 s"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let albums: Value = client
        .get("/get/get-albums")
        .cookie(admin())
        .dispatch()
        .into_json()
        .expect("albums json");
    let album_id = albums
        .as_array()
        .expect("albums array")
        .iter()
        .find(|a| a["dirPath"].as_str() == Some("sharecfg"))
        .unwrap_or_else(|| panic!("no album for sharecfg, got: {albums}"))["albumId"]
        .as_str()
        .expect("albumId")
        .to_owned();
    let share_res = client
        .post("/post/create_share")
        .cookie(admin())
        .header(ContentType::JSON)
        .body(
            serde_json::json!({
                "albumId": album_id,
                "description": "tier test",
                "password": null,
                "showMetadata": true,
                "showDownload": false,
                "showUpload": false,
                "exp": 0
            })
            .to_string(),
        )
        .dispatch();
    assert_eq!(share_res.status(), Status::Ok, "create_share");
    let share_id = share_res.into_string().expect("share id").trim().to_owned();

    // ── (a) share headers, no cookie: 200, null paths, no host keys ──
    let share_json: Value = client
        .get("/get/config")
        .header(Header::new("x-album-id", album_id.clone()))
        .header(Header::new("x-share-id", share_id.clone()))
        .dispatch()
        .into_json()
        .expect("share config json");
    assert!(
        share_json.get("address").is_none(),
        "share tier must not see address, got: {share_json}"
    );
    assert!(
        share_json.get("port").is_none(),
        "share tier must not see port, got: {share_json}"
    );
    assert!(
        share_json.get("imagePath").is_some_and(Value::is_null),
        "share tier imagePath must be null, got: {share_json}"
    );
    assert!(
        share_json.get("uploadFolder").is_some_and(Value::is_null),
        "share tier uploadFolder must be null, got: {share_json}"
    );

    // ── (b) admin session: 200, real values, no host keys ────────────
    let admin_json: Value = client
        .get("/get/config")
        .cookie(admin())
        .dispatch()
        .into_json()
        .expect("admin config json");
    assert!(
        admin_json.get("address").is_none(),
        "admin tier must not see address, got: {admin_json}"
    );
    assert!(
        admin_json.get("port").is_none(),
        "admin tier must not see port, got: {admin_json}"
    );
    assert!(
        admin_json.get("imagePath").is_some_and(Value::is_string),
        "admin tier imagePath must be the real path, got: {admin_json}"
    );
    assert_eq!(
        admin_json["uploadFolder"].as_str(),
        Some("env_folder"),
        "admin tier uploadFolder must be the real value, got: {admin_json}"
    );

    // ── (c) non-admin session: GuardShare admits admins and shares ──
    // only, so a non-admin cookie is refused before the handler's tier
    // rule runs (the handler's `User { admin: false }` arm stays as
    // unreachable defense-in-depth).
    let viewer_res = client.get("/get/config").cookie(viewer()).dispatch();
    assert_eq!(
        viewer_res.status(),
        Status::Unauthorized,
        "non-admin session must not pass GuardShare"
    );
}
