use anyhow::Context;
use base64::{Engine as _, engine::general_purpose};
use log::{info, warn};
use rand::{TryRng, rngs::SysRng};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

use crate::storage::files::{DATA_PATH, get_config_path, get_data_path, resolve_root};

pub static FALLBACK_SECRET_KEY: OnceLock<String> = OnceLock::new();

fn generate_secret_key() -> String {
    let mut secret = vec![0u8; 32];
    SysRng
        .try_fill_bytes(&mut secret)
        .expect("Failed to generate random secret key");
    general_purpose::STANDARD.encode(secret)
}

fn default_true() -> bool {
    true
}

fn default_upload_folder() -> String {
    "uploads".to_string()
}

fn default_max_upload_size() -> String {
    "100MiB".to_string()
}

// ── Namespace config ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
pub struct NamespaceConfig {
    pub name: String,
    #[schema(value_type = String)]
    pub path: PathBuf,
}

// ── JSON API format (camelCase) ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(utoipa::ToSchema)]
#[allow(clippy::struct_excessive_bools)]
pub struct AppConfig {
    pub address: String,
    pub port: u16,
    #[schema(value_type = Option<String>)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_home: Option<PathBuf>,
    #[serde(default = "default_upload_folder")]
    pub upload_folder: String,
    #[serde(default = "default_max_upload_size")]
    pub max_upload_size: String,
    pub read_only_mode: bool,
    pub disable_img: bool,
    #[serde(default = "default_true")]
    pub fs_notify_watcher: bool,
    /// NFC-normalize uploaded filenames so macOS NFD names collapse onto
    /// their composed form. Optional (unlike the always-on sanitization tiers).
    #[serde(default = "default_true")]
    pub normalize_upload_filenames: bool,
    /// Cross-check uploads against the declared `Content-Type`. When enabled,
    /// the first 512 bytes of each uploaded file are sniffed with the
    /// [`infer`](https://crates.io/crates/infer) magic-byte database and the
    /// detected signature must fall in the family of the extension derived
    /// from the `Content-Type`: `jpg|jpeg|jfif|jpe` → JPEG, `tif|tiff` →
    /// TIFF, `mp4|mov|m4v` → ISO BMFF, `mkv|webm` → EBML, `mpeg` → MPEG-PS,
    /// and `png`, `webp`, `bmp`, `gif`, `avi`, `flv`, `wmv` 1:1. Mismatches
    /// and unrecognizable bytes are rejected with `400 InvalidInput`; the
    /// check is signature-based only, never a full decode, so unusual-but-valid
    /// variants still pass. The stored file extension remains the one derived
    /// from the declared `Content-Type`. See
    /// `backend/src/router/post/post_upload.rs` (`validate_upload_content`).
    /// Disable only if legitimate media is being rejected.
    #[serde(default = "default_true")]
    pub validate_upload_content: bool,
    /// Trust the `lastModified` the upload client sends with each file. When
    /// enabled, the value is used as the stored file modification time but
    /// clamped to `[1970-01-01, now + 24h]` so a broken client clock cannot
    /// push an undated file (e.g. a screenshot) into year 1970 or the distant
    /// future. Disabled by default: the server uses `now()` instead of the
    /// provided value, since a client's clock or timezone cannot be relied on.
    /// Only affects files without embedded metadata: photos with a
    /// `DateTimeOriginal` EXIF tag keep their EXIF-derived date regardless.
    /// See `backend/src/router/post/post_upload.rs`
    /// (`resolve_upload_timestamp`).
    #[serde(default)]
    pub use_client_timestamp_info: bool,
    #[serde(default = "default_true")]
    pub trash_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_key: Option<String>,
    /// Path to the compiled frontend build directory (contains index.html and assets/).
    /// Not exposed via the JSON API; set via config.toml or `PICASU_WEB_ROOT` env var.
    #[serde(skip)]
    pub web_root: Option<PathBuf>,
    #[serde(default)]
    pub namespaces: Vec<NamespaceConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            address: "0.0.0.0".to_string(),
            port: 5673,
            data_home: None,
            upload_folder: default_upload_folder(),
            max_upload_size: default_max_upload_size(),
            read_only_mode: false,
            disable_img: false,
            fs_notify_watcher: true,
            normalize_upload_filenames: true,
            validate_upload_content: true,
            use_client_timestamp_info: false,
            trash_enabled: true,
            password: None,
            auth_key: None,
            web_root: None,
            namespaces: Vec::new(),
        }
    }
}

// ── TOML file format (snake_case, sections) ──────────────────────────────────

#[derive(Serialize, Deserialize)]
pub(crate) struct TomlFile {
    #[serde(default)]
    pub(crate) server: TomlServer,
    #[serde(default)]
    pub(crate) gallery: TomlGallery,
    #[serde(default)]
    pub(crate) secrets: TomlSecrets,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct TomlServer {
    #[serde(default = "default_address")]
    pub(crate) address: String,
    #[serde(default = "default_port")]
    pub(crate) port: u16,
    #[serde(default = "default_max_upload_size")]
    pub(crate) max_upload_size: String,
}

impl Default for TomlServer {
    fn default() -> Self {
        Self {
            address: default_address(),
            port: default_port(),
            max_upload_size: default_max_upload_size(),
        }
    }
}

fn default_address() -> String {
    "0.0.0.0".to_string()
}
fn default_port() -> u16 {
    5673
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct TomlGallery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) data_home: Option<PathBuf>,
    #[serde(default = "default_upload_folder")]
    pub(crate) upload_folder: String,
    #[serde(default)]
    pub(crate) read_only_mode: bool,
    #[serde(default)]
    pub(crate) disable_img: bool,
    #[serde(default = "default_true")]
    pub(crate) fs_notify_watcher: bool,
    #[serde(default = "default_true")]
    pub(crate) normalize_upload_filenames: bool,
    #[serde(default = "default_true")]
    pub(crate) validate_upload_content: bool,
    #[serde(default)]
    pub(crate) use_client_timestamp_info: bool,
    #[serde(default = "default_true")]
    pub(crate) trash_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) web_root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) namespaces: Vec<NamespaceConfig>,
}

impl Default for TomlGallery {
    fn default() -> Self {
        Self {
            data_home: None,
            upload_folder: default_upload_folder(),
            read_only_mode: false,
            disable_img: false,
            fs_notify_watcher: true,
            normalize_upload_filenames: true,
            validate_upload_content: true,
            use_client_timestamp_info: false,
            trash_enabled: true,
            web_root: None,
            namespaces: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) struct TomlSecrets {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) auth_key: Option<String>,
}

impl From<TomlFile> for AppConfig {
    fn from(t: TomlFile) -> Self {
        AppConfig {
            address: t.server.address,
            port: t.server.port,
            data_home: t.gallery.data_home,
            upload_folder: t.gallery.upload_folder,
            max_upload_size: t.server.max_upload_size,
            read_only_mode: t.gallery.read_only_mode,
            disable_img: t.gallery.disable_img,
            fs_notify_watcher: t.gallery.fs_notify_watcher,
            normalize_upload_filenames: t.gallery.normalize_upload_filenames,
            validate_upload_content: t.gallery.validate_upload_content,
            use_client_timestamp_info: t.gallery.use_client_timestamp_info,
            trash_enabled: t.gallery.trash_enabled,
            password: t.secrets.password,
            auth_key: t.secrets.auth_key,
            web_root: t.gallery.web_root,
            namespaces: t.gallery.namespaces,
        }
    }
}

impl From<AppConfig> for TomlFile {
    fn from(c: AppConfig) -> Self {
        TomlFile {
            server: TomlServer {
                address: c.address,
                port: c.port,
                max_upload_size: c.max_upload_size,
            },
            gallery: TomlGallery {
                data_home: c.data_home,
                upload_folder: c.upload_folder,
                read_only_mode: c.read_only_mode,
                disable_img: c.disable_img,
                fs_notify_watcher: c.fs_notify_watcher,
                normalize_upload_filenames: c.normalize_upload_filenames,
                validate_upload_content: c.validate_upload_content,
                use_client_timestamp_info: c.use_client_timestamp_info,
                trash_enabled: c.trash_enabled,
                web_root: c.web_root,
                namespaces: c.namespaces,
            },
            secrets: TomlSecrets {
                password: c.password,
                auth_key: c.auth_key,
            },
        }
    }
}

// ── Global config store ──────────────────────────────────────────────────────

pub static APP_CONFIG: OnceLock<RwLock<AppConfig>> = OnceLock::new();

impl AppConfig {
    pub fn get_jwt_secret_key(&self) -> Vec<u8> {
        match self.auth_key.as_ref() {
            Some(key) => key.as_bytes().to_vec(),
            None => FALLBACK_SECRET_KEY
                .get_or_init(generate_secret_key)
                .as_bytes()
                .to_vec(),
        }
    }

    /// Validate namespace configuration:
    /// - At least one namespace configured, names unique
    /// - When `trash_enabled` is true, a namespace named "trash" must exist
    /// - All namespace roots on the same filesystem (`st_dev`), else fail fast
    fn validate_namespaces(&self) -> anyhow::Result<()> {
        use std::collections::HashSet;

        if self.namespaces.is_empty() {
            anyhow::bail!("at least one [[namespace]] must be configured");
        }

        let mut seen = HashSet::new();
        for ns in &self.namespaces {
            if !seen.insert(&ns.name) {
                anyhow::bail!("duplicate namespace name: \"{}\"", ns.name);
            }
            if !ns.path.exists() {
                // Create the directory if it doesn't exist
                std::fs::create_dir_all(&ns.path).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to create namespace \"{}\" path {}: {e}",
                        ns.name,
                        ns.path.display()
                    )
                })?;
            }
        }

        if self.trash_enabled && !self.namespaces.iter().any(|ns| ns.name == "trash") {
            anyhow::bail!("trash_enabled is true but no namespace named \"trash\" is configured");
        }

        // All namespace roots must be on the same filesystem
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let mut dev: Option<u64> = None;
            for ns in &self.namespaces {
                let metadata = std::fs::metadata(&ns.path).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to stat namespace \"{}\" path {}: {e}",
                        ns.name,
                        ns.path.display()
                    )
                })?;
                match dev {
                    Some(d) if d != metadata.dev() => {
                        anyhow::bail!(
                            "namespace \"{}\" ({}) is on a different filesystem than other namespaces (dev {} vs {})",
                            ns.name,
                            ns.path.display(),
                            metadata.dev(),
                            d
                        );
                    }
                    None => dev = Some(metadata.dev()),
                    _ => {}
                }
            }
        }

        Ok(())
    }

    /// # Panics
    /// Panics if the global configuration is already initialized.
    pub fn init() {
        let config_path = get_config_path();
        let config_path_display = config_path.display();

        // First-launch: config.toml doesn't exist yet
        if !config_path.exists() {
            info!(
                "Configuration file not found at {config_path_display}. Creating default config.toml..."
            );

            let data_home =
                resolve_root("PICASU_DATA_HOME", "data", |p| p.data_dir().to_path_buf());

            let web_root = match std::env::var("PICASU_WEB_ROOT") {
                Ok(p) => Some(PathBuf::from(p)),
                Err(_) => Some(data_home.join("www")),
            };

            let shared_path = data_home.join("images");
            let trash_path = data_home.join("trash");
            // Create default namespace directories so validation passes
            for p in [&shared_path, &trash_path] {
                if let Err(e) = fs::create_dir_all(p) {
                    warn!(
                        "Failed to create default namespace directory {}: {e}",
                        p.display()
                    );
                }
            }
            let namespaces = vec![
                NamespaceConfig {
                    name: "shared".to_string(),
                    path: shared_path,
                },
                NamespaceConfig {
                    name: "trash".to_string(),
                    path: trash_path,
                },
            ];

            let config = AppConfig {
                data_home: Some(data_home),
                web_root,
                namespaces,
                ..AppConfig::default()
            };

            if let Some(parent) = config_path.parent()
                && let Err(e) = fs::create_dir_all(parent)
            {
                warn!(
                    "Failed to create config directory {}: {e}",
                    parent.display()
                );
            }

            if let Err(e) = Self::save_update(&config) {
                warn!("Failed to create default config file: {e}");
            } else {
                info!("Default configuration created successfully.");
            }
        }

        // Read config file via TOML wrapper and merge with defaults
        info!("Loading configuration from {config_path_display}");

        let mut config = match fs::read_to_string(&config_path) {
            Ok(content) => match toml::from_str::<TomlFile>(&content) {
                Ok(tf) => AppConfig::from(tf),
                Err(e) => {
                    warn!("Failed to parse config at {config_path_display}: {e:?}, using defaults");
                    AppConfig::default()
                }
            },
            Err(e) => {
                warn!("Failed to read config file {config_path_display}: {e}");
                AppConfig::default()
            }
        };

        // Overwrite file if we had to fall back to defaults
        if config == AppConfig::default()
            && config_path.exists()
            && let Err(e) = Self::save_update(&config)
        {
            warn!("Failed to save default config: {e}");
        }

        Self::apply_env_overrides(&mut config);

        if config.web_root.is_none() {
            let data_home = config
                .data_home
                .clone()
                .unwrap_or_else(|| get_data_path().clone());
            config.web_root = Some(data_home.join("www"));
        }

        {
            let data_path = config
                .data_home
                .clone()
                .unwrap_or_else(|| get_data_path().clone());
            DATA_PATH.set(data_path).ok();
        }

        if config.auth_key.as_deref().is_none_or(str::is_empty) {
            config.auth_key = None;
            FALLBACK_SECRET_KEY.get_or_init(generate_secret_key);
        }

        // Validate namespace configuration
        if let Err(e) = config.validate_namespaces() {
            panic!("Namespace configuration error: {e}");
        }

        APP_CONFIG
            .set(RwLock::new(config))
            .expect("Config already initialized");

        // Let the logger know where we ended up
        info!("Configuration loaded successfully.");
    }

    fn apply_env_overrides(config: &mut AppConfig) {
        if let Ok(val) = std::env::var("PICASU_DATA_HOME") {
            let p = val.trim().to_string();
            if !p.is_empty() {
                config.data_home = Some(PathBuf::from(p));
            }
        }
        if let Ok(val) = std::env::var("PICASU_PORT")
            && let Ok(port) = val.parse()
        {
            config.port = port;
        }
        if let Ok(val) = std::env::var("PICASU_ADDRESS") {
            config.address = val;
        }
        if let Ok(val) = std::env::var("PICASU_READ_ONLY_MODE")
            && let Ok(mode) = val.parse()
        {
            config.read_only_mode = mode;
        }
        if let Ok(val) = std::env::var("PICASU_DISABLE_IMG")
            && let Ok(disabled) = val.parse()
        {
            config.disable_img = disabled;
        }
        if let Ok(val) = std::env::var("PICASU_FS_NOTIFY_WATCHER") {
            if let Ok(enabled) = val.parse() {
                config.fs_notify_watcher = enabled;
            } else {
                warn!("PICASU_FS_NOTIFY_WATCHER='{val}' is not 'true' or 'false', ignoring");
            }
        }
        if let Ok(val) = std::env::var("PICASU_UPLOAD_FOLDER") {
            let trimmed = val.trim().to_string();
            if !trimmed.is_empty() {
                config.upload_folder = trimmed;
            }
        }
        if let Ok(val) = std::env::var("PICASU_MAX_UPLOAD_SIZE") {
            let trimmed = val.trim().to_string();
            if !trimmed.is_empty() {
                config.max_upload_size = trimmed;
            }
        }
        if let Ok(val) = std::env::var("PICASU_AUTH_KEY") {
            let trimmed = val.trim().to_string();
            config.auth_key = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            };
        }
        if let Ok(val) = std::env::var("PICASU_WEB_ROOT") {
            let trimmed = val.trim().to_string();
            if !trimmed.is_empty() {
                config.web_root = Some(PathBuf::from(trimmed));
            }
        }
    }

    /// # Panics
    /// Panics if the global configuration has not been initialized.
    /// # Errors
    /// Returns an error if the configuration cannot be serialized and saved to disk.
    pub fn update(mut new_config: AppConfig) -> anyhow::Result<()> {
        use crate::tasks::batcher::start_watcher::reload_watcher;

        info!("Updating configuration...");

        new_config.upload_folder = new_config.upload_folder.trim().to_string();

        if new_config.auth_key.as_deref().is_none_or(str::is_empty) {
            new_config.auth_key = None;
        }

        Self::save_update(&new_config).context("Failed to save configuration to file")?;

        {
            let mut w = APP_CONFIG
                .get()
                .expect("APP_CONFIG not initialized")
                .write()
                .expect("lock poisoned");
            if new_config.auth_key.is_none() {
                FALLBACK_SECRET_KEY.get_or_init(generate_secret_key);
            }
            *w = new_config.clone();
        }

        reload_watcher();
        info!("Configuration updated successfully");
        Ok(())
    }

    fn save_update(config: &AppConfig) -> anyhow::Result<()> {
        let config_path = get_config_path();
        let config_path_display = config_path.display();

        let toml_file = TomlFile::from(config.clone());
        let pretty_toml = toml::to_string_pretty(&toml_file)
            .context("Failed to serialize configuration to TOML")?;

        fs::write(&config_path, pretty_toml.as_bytes()).context(format!(
            "Failed to write configuration to {config_path_display}"
        ))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_round_trip_full() {
        let config = AppConfig {
            address: "127.0.0.1".to_string(),
            port: 8080,
            data_home: Some(PathBuf::from("/tmp/data")),
            upload_folder: "test_uploads".to_string(),
            max_upload_size: "500MiB".to_string(),
            read_only_mode: true,
            disable_img: false,
            fs_notify_watcher: false,
            normalize_upload_filenames: false,
            validate_upload_content: true,
            use_client_timestamp_info: false,
            trash_enabled: true,
            password: Some("secret".to_string()),
            auth_key: None,
            web_root: Some(PathBuf::from("/tmp/www")),
            namespaces: Vec::new(),
        };

        let tf = TomlFile::from(config.clone());
        let toml_str = toml::to_string_pretty(&tf).expect("failed to serialize toml");
        let parsed: TomlFile = toml::from_str(&toml_str).expect("failed to deserialize toml");
        let restored = AppConfig::from(parsed);
        assert_eq!(config, restored);
    }

    #[test]
    fn toml_sections_are_correct() {
        let config = AppConfig {
            address: "10.0.0.1".to_string(),
            port: 8000,
            data_home: Some(PathBuf::from("/data")),
            max_upload_size: "200MiB".to_string(),
            password: Some("hunter2".to_string()),
            auth_key: Some("jwt-secret".to_string()),
            ..AppConfig::default()
        };

        let tf = TomlFile::from(config);
        let toml_str = toml::to_string_pretty(&tf).expect("failed to serialize toml");

        assert!(
            toml_str.contains("[server]"),
            "should have [server] section"
        );
        assert!(
            toml_str.contains("[gallery]"),
            "should have [gallery] section"
        );
        assert!(
            toml_str.contains("[secrets]"),
            "should have [secrets] section"
        );
        assert!(toml_str.contains("address = \"10.0.0.1\""));
        assert!(toml_str.contains("port = 8000"));
        assert!(toml_str.contains("max_upload_size = \"200MiB\""));
        assert!(toml_str.contains("data_home = \"/data\""));
        assert!(toml_str.contains("password = \"hunter2\""));
        assert!(toml_str.contains("auth_key = \"jwt-secret\""));
    }

    #[test]
    fn trash_config_defaults() {
        let config = AppConfig::default();
        assert!(config.trash_enabled, "trash_enabled should default to true");
    }

    #[test]
    fn trash_config_from_toml_defaults() {
        let toml_str = r#"
[server]
port = 5673
"#;
        let parsed: TomlFile = toml::from_str(toml_str).expect("failed to deserialize toml");
        let config = AppConfig::from(parsed);
        assert!(config.trash_enabled);
    }

    #[test]
    fn trash_config_toml_round_trip() {
        let config = AppConfig {
            trash_enabled: false,
            ..AppConfig::default()
        };
        let tf = TomlFile::from(config.clone());
        let toml_str = toml::to_string_pretty(&tf).expect("serialize");
        let parsed: TomlFile = toml::from_str(&toml_str).expect("deserialize");
        let restored = AppConfig::from(parsed);
        assert_eq!(config, restored);
    }

    #[test]
    fn toml_defaults_fill_gaps() {
        let toml_str = r#"
[server]
port = 9999

[gallery]
read_only_mode = true
"#;
        let parsed: TomlFile = toml::from_str(toml_str).expect("failed to deserialize toml");
        let config = AppConfig::from(parsed);

        assert_eq!(config.port, 9999);
        assert!(config.read_only_mode);
        assert_eq!(config.address, "0.0.0.0");
        assert_eq!(config.max_upload_size, "100MiB");
        assert_eq!(config.upload_folder, "uploads");
        assert!(!config.disable_img);
    }

    #[test]
    fn namespace_config_toml_round_trip() {
        let config = AppConfig {
            namespaces: vec![
                NamespaceConfig {
                    name: "shared".to_string(),
                    path: PathBuf::from("/mnt/photos/shared"),
                },
                NamespaceConfig {
                    name: "trash".to_string(),
                    path: PathBuf::from("/mnt/photos/trash"),
                },
            ],
            ..AppConfig::default()
        };
        let tf = TomlFile::from(config.clone());
        let toml_str = toml::to_string_pretty(&tf).expect("serialize");
        assert!(toml_str.contains("[[gallery.namespaces]]"));
        let parsed: TomlFile = toml::from_str(&toml_str).expect("deserialize");
        let restored = AppConfig::from(parsed);
        assert_eq!(config, restored);
    }

    #[test]
    fn namespace_toml_parsing() {
        let toml_str = r#"
[server]
port = 5673

[[gallery.namespaces]]
name = "shared"
path = "/mnt/photos/shared"

[[gallery.namespaces]]
name = "trash"
path = "/mnt/photos/trash"
"#;
        let parsed: TomlFile = toml::from_str(toml_str).expect("failed to deserialize toml");
        let config = AppConfig::from(parsed);
        assert_eq!(config.namespaces.len(), 2);
        assert_eq!(config.namespaces[0].name, "shared");
        assert_eq!(
            config.namespaces[0].path,
            PathBuf::from("/mnt/photos/shared")
        );
        assert_eq!(config.namespaces[1].name, "trash");
        assert_eq!(
            config.namespaces[1].path,
            PathBuf::from("/mnt/photos/trash")
        );
    }

    #[test]
    fn validate_namespaces_empty_fails() {
        let config = AppConfig::default();
        let result = config.validate_namespaces();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("at least one"));
    }

    #[test]
    fn validate_namespaces_duplicate_names_fails() {
        let dir = tempfile::tempdir().unwrap();
        let config = AppConfig {
            namespaces: vec![
                NamespaceConfig {
                    name: "shared".to_string(),
                    path: dir.path().to_path_buf(),
                },
                NamespaceConfig {
                    name: "shared".to_string(),
                    path: dir.path().to_path_buf(),
                },
            ],
            ..AppConfig::default()
        };
        let result = config.validate_namespaces();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("duplicate"));
    }

    #[test]
    fn validate_namespaces_missing_path_creates_dir() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nonexistent");
        assert!(!missing.exists());
        let config = AppConfig {
            trash_enabled: false,
            namespaces: vec![NamespaceConfig {
                name: "shared".to_string(),
                path: missing.clone(),
            }],
            ..AppConfig::default()
        };
        assert!(config.validate_namespaces().is_ok());
        assert!(missing.exists(), "namespace directory should be created");
    }

    #[test]
    fn validate_namespaces_trash_enabled_no_trash_fails() {
        let dir = tempfile::tempdir().unwrap();
        let config = AppConfig {
            trash_enabled: true,
            namespaces: vec![NamespaceConfig {
                name: "shared".to_string(),
                path: dir.path().to_path_buf(),
            }],
            ..AppConfig::default()
        };
        let result = config.validate_namespaces();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("trash_enabled is true")
        );
    }

    #[test]
    fn validate_namespaces_trash_disabled_ok() {
        let dir = tempfile::tempdir().unwrap();
        let config = AppConfig {
            trash_enabled: false,
            namespaces: vec![NamespaceConfig {
                name: "shared".to_string(),
                path: dir.path().to_path_buf(),
            }],
            ..AppConfig::default()
        };
        assert!(config.validate_namespaces().is_ok());
    }

    #[test]
    fn validate_namespaces_same_fs_ok() {
        let dir = tempfile::tempdir().unwrap();
        let sub1 = dir.path().join("ns1");
        let sub2 = dir.path().join("ns2");
        std::fs::create_dir_all(&sub1).unwrap();
        std::fs::create_dir_all(&sub2).unwrap();
        let config = AppConfig {
            trash_enabled: false,
            namespaces: vec![
                NamespaceConfig {
                    name: "a".to_string(),
                    path: sub1,
                },
                NamespaceConfig {
                    name: "b".to_string(),
                    path: sub2,
                },
            ],
            ..AppConfig::default()
        };
        assert!(config.validate_namespaces().is_ok());
    }
}
