use picasu::AppConfig;

#[test]
fn api_first_launch_aborts_on_explicit_empty_users_home() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cfg_dir = dir.path().join("config");
    let dat_dir = dir.path().join("data");
    std::fs::create_dir_all(&cfg_dir).expect("create cfg dir");
    std::fs::create_dir_all(&dat_dir).expect("create data dir");
    std::fs::write(
        cfg_dir.join("config.toml"),
        format!(
            r#"
[gallery]
data_home = "{}"
users_home = ""
"#,
            dat_dir.display()
        ),
    )
    .expect("write config");

    unsafe {
        std::env::set_var("PICASU_CONFIG_HOME", cfg_dir.to_str().expect("utf8"));
        std::env::set_var("PICASU_DATA_HOME", dat_dir.to_str().expect("utf8"));
    }

    let res = AppConfig::init();
    assert!(res.is_err(), "explicit empty users_home must abort boot");

    unsafe {
        std::env::remove_var("PICASU_CONFIG_HOME");
        std::env::remove_var("PICASU_DATA_HOME");
    }
}
