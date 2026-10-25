use picasu::{APP_CONFIG, AppConfig};

/// Verify first-launch path resolution when no config.toml exists.
///
/// With `PICASU_DATA_HOME` and `PICASU_IMAGE_HOME` set, those env values
/// should be written into config.toml and loaded into `AppConfig.data_home`
/// and `image_home`.
#[test]
fn api_first_launch_resolves_paths() {
    let dir = tempfile::tempdir().expect("temp dir for first-launch config");
    let cfg_dir = dir.path().join("config");
    let dat_dir = dir.path().join("data");
    let img_dir = dir.path().join("my_images");
    std::fs::create_dir_all(&cfg_dir).expect("create PICASU_CONFIG_HOME dir");

    unsafe {
        std::env::set_var(
            "PICASU_CONFIG_HOME",
            cfg_dir.to_str().expect("config home path is UTF-8"),
        );
        std::env::set_var(
            "PICASU_DATA_HOME",
            dat_dir.to_str().expect("data home path is UTF-8"),
        );
        std::env::set_var(
            "PICASU_IMAGE_HOME",
            img_dir.to_str().expect("image home path is UTF-8"),
        );
    }

    // No config.toml exists — first launch should create it
    assert!(!cfg_dir.join("config.toml").exists());
    AppConfig::init().expect("first-launch AppConfig::init() must succeed");
    assert!(
        cfg_dir.join("config.toml").exists(),
        "config.toml should be created"
    );

    let cfg = APP_CONFIG
        .get()
        .expect("APP_CONFIG installed by AppConfig::init()")
        .read()
        .expect("config lock not poisoned");
    assert_eq!(
        cfg.data_home.as_deref(),
        Some(dat_dir.as_path()),
        "data_home from env"
    );
    assert_eq!(
        cfg.image_home.as_deref(),
        Some(img_dir.as_path()),
        "image_home from env"
    );
    assert_eq!(cfg.port, 5673, "default port");
    assert_eq!(cfg.upload_folder, "uploads", "default upload_folder");

    unsafe {
        std::env::remove_var("PICASU_CONFIG_HOME");
        std::env::remove_var("PICASU_DATA_HOME");
        std::env::remove_var("PICASU_IMAGE_HOME");
    }
}
