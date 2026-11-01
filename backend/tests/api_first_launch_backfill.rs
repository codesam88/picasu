use picasu::{APP_CONFIG, AppConfig};

#[test]
fn api_first_launch_backfills_users_home() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cfg_dir = dir.path().join("config");
    let dat_dir = dir.path().join("data");
    std::fs::create_dir_all(&cfg_dir).expect("create cfg dir");
    std::fs::create_dir_all(&dat_dir).expect("create data dir");
    std::fs::write(
        cfg_dir.join("config.toml"),
        format!(
            r#"
[server]
port = 5673

[gallery]
data_home = "{}"
image_home = "{}"
"#,
            dat_dir.display(),
            dat_dir.join("images").display()
        ),
    )
    .expect("write config");

    unsafe {
        std::env::set_var("PICASU_CONFIG_HOME", cfg_dir.to_str().expect("utf8"));
        std::env::set_var("PICASU_DATA_HOME", dat_dir.to_str().expect("utf8"));
    }

    AppConfig::init().expect("init must succeed");
    let cfg = APP_CONFIG.get().expect("cfg").read().expect("lock");
    assert_eq!(
        cfg.users_home.as_deref(),
        Some(dat_dir.join("users").as_path())
    );
    let raw = std::fs::read_to_string(cfg_dir.join("config.toml")).expect("read");
    assert!(raw.contains("users_home"), "backfilled into file: {raw}");

    unsafe {
        std::env::remove_var("PICASU_CONFIG_HOME");
        std::env::remove_var("PICASU_DATA_HOME");
    }
}
