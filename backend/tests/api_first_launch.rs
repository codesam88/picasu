use picasu::{APP_CONFIG, AppConfig};

/// Verify first-launch path resolution when no config.toml exists.
///
/// With PICASU_DATA_HOME set, that env value should be written into
/// config.toml and loaded into AppConfig.data_home. A default "shared"
/// namespace should be created pointing to data_home/images.
#[test]
fn api_first_launch_resolves_paths() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_dir = dir.path().join("config");
    let dat_dir = dir.path().join("data");
    std::fs::create_dir_all(&cfg_dir).unwrap();

    unsafe {
        std::env::set_var("PICASU_CONFIG_HOME", cfg_dir.to_str().unwrap());
        std::env::set_var("PICASU_DATA_HOME", dat_dir.to_str().unwrap());
    }

    // No config.toml exists — first launch should create it
    assert!(!cfg_dir.join("config.toml").exists());
    AppConfig::init();
    assert!(
        cfg_dir.join("config.toml").exists(),
        "config.toml should be created"
    );

    let cfg = APP_CONFIG.get().unwrap().read().unwrap();
    assert_eq!(
        cfg.data_home.as_deref(),
        Some(dat_dir.as_path()),
        "data_home from env"
    );
    assert_eq!(
        cfg.namespaces.len(),
        2,
        "default shared and trash namespaces should be created"
    );
    assert_eq!(cfg.namespaces[0].name, "shared");
    assert_eq!(cfg.namespaces[0].path, dat_dir.join("images"));
    assert_eq!(cfg.namespaces[1].name, "trash");
    assert_eq!(cfg.namespaces[1].path, dat_dir.join("trash"));
    assert_eq!(cfg.port, 5673, "default port");
    assert_eq!(cfg.upload_folder, "uploads", "default upload_folder");

    unsafe {
        std::env::remove_var("PICASU_CONFIG_HOME");
        std::env::remove_var("PICASU_DATA_HOME");
    }
}
