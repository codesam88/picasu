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

pub(crate) fn generate_secret_key() -> String {
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

// Login-throttle defaults, pinned to the shipped schedules in
// `crate::auth::rate_limit` so the config cannot drift from the constants
// the bucket oracle tests pin. All three buckets reject degenerate values
// at load (`AppConfig::validate_login_throttle`).

fn default_login_ip_window_secs() -> u64 {
    crate::auth::rate_limit::LOGIN_IP_WINDOW_SECS
}
fn default_login_ip_max_failures() -> u32 {
    crate::auth::rate_limit::LOGIN_IP_MAX_FAILURES
}
fn default_login_ip_base_ban_secs() -> u64 {
    crate::auth::rate_limit::LOGIN_IP_BASE_BAN_SECS
}
fn default_login_ip_max_ban_secs() -> u64 {
    crate::auth::rate_limit::LOGIN_IP_MAX_BAN_SECS
}
fn default_login_ip_long_window_secs() -> u64 {
    crate::auth::rate_limit::LOGIN_IP_LONG_WINDOW_SECS
}
fn default_login_ip_long_max_failures() -> u32 {
    crate::auth::rate_limit::LOGIN_IP_LONG_MAX_FAILURES
}
fn default_login_ip_long_base_ban_secs() -> u64 {
    crate::auth::rate_limit::LOGIN_IP_LONG_BASE_BAN_SECS
}
fn default_login_ip_long_max_ban_secs() -> u64 {
    crate::auth::rate_limit::LOGIN_IP_LONG_MAX_BAN_SECS
}
fn default_login_user_long_window_secs() -> u64 {
    crate::auth::rate_limit::LOGIN_USER_LONG_WINDOW_SECS
}
fn default_login_user_long_max_failures() -> u32 {
    crate::auth::rate_limit::LOGIN_USER_LONG_MAX_FAILURES
}
fn default_login_user_long_base_ban_secs() -> u64 {
    crate::auth::rate_limit::LOGIN_USER_LONG_BASE_BAN_SECS
}
fn default_login_user_long_max_ban_secs() -> u64 {
    crate::auth::rate_limit::LOGIN_USER_LONG_MAX_BAN_SECS
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
    #[schema(value_type = Option<String>)]
    #[serde(rename = "imagePath", alias = "imageHome")]
    pub image_home: Option<PathBuf>,
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
    /// Cross-check uploaded content against the type its declared
    /// `Content-Type` implies. The declared extension yields the expected
    /// format from the supported-format table; the content is then identified
    /// and the two are compared. See `backend/src/process/format.rs`.
    ///
    /// Content that cannot be identified at all is always rejected, with or
    /// without this setting: there is no way to tell whether such bytes are
    /// what the extension claims. This setting governs only the mismatch case,
    /// where the content *is* identified but is not the type the extension
    /// claims — for example a video uploaded with an image content type, or an
    /// image named with the wrong extension. Enabled (the default) rejects
    /// those with `400 InvalidInput`; disabling it stores the file anyway, which
    /// misdescribes it and leaves the indexer trusting a wrong extension. Only
    /// disable this to tolerate mislabeled files.
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
    /// Allow self-registration via `POST /post/users/register` without an
    /// invite token. Default off: registration then requires a valid invite.
    #[serde(default)]
    pub allow_registration: bool,
    /// Disable the default password-strength checks (length floor/ceiling,
    /// blocklist, user-id and pattern rules) for new passwords. Default
    /// off. Only for development, throwaway installs, or tests that must
    /// use weak fixtures — the bypass is warn-logged at first use.
    #[serde(default)]
    pub trivial_passwords: bool,
    /// Enable the online first-admin bootstrap window. While set **and the
    /// user store is empty**, unauthenticated `POST /post/users/bootstrap`
    /// enrolls the first admin; until then `POST /post/users/authenticate` takes
    /// the unknown-user path and every guarded route refuses with
    /// `No administrator enrolled` (the refusal names the bootstrap
    /// endpoint while the window is open). Default
    /// off: a fresh install answers 401 everywhere until an operator opts
    /// in (or provisions the store via the `picasu-admin` CLI). Enabling
    /// the flag against an existing `passwd.json` makes the server refuse
    /// to start. WARN-logged at boot whenever set.
    #[serde(default)]
    pub bootstrap_admin: bool,
    /// Trust the `X-Forwarded-For` header when keying the login throttle
    /// (leftmost entry, only when it parses as an IP address). Default off:
    /// the header is client-spoofable, so only a reverse-proxy deployment
    /// that strips or overwrites it may enable this. Host-only (restart
    /// required).
    #[serde(default)]
    pub trust_proxy_forwarded_for: bool,
    /// Login throttle IP-short burst bucket: `login_ip_max_failures`
    /// failures within `login_ip_window_secs` open a ban of
    /// `login_ip_base_ban_secs`, doubling per further pair of failures up
    /// to `login_ip_max_ban_secs`. Host-only (restart required).
    #[serde(default = "default_login_ip_window_secs")]
    pub login_ip_window_secs: u64,
    /// See [`Self::login_ip_window_secs`].
    #[serde(default = "default_login_ip_max_failures")]
    pub login_ip_max_failures: u32,
    /// See [`Self::login_ip_window_secs`].
    #[serde(default = "default_login_ip_base_ban_secs")]
    pub login_ip_base_ban_secs: u64,
    /// See [`Self::login_ip_window_secs`].
    #[serde(default = "default_login_ip_max_ban_secs")]
    pub login_ip_max_ban_secs: u64,
    /// Login throttle IP-long daily bucket: paced spraying ceiling across
    /// many usernames from one source. Host-only (restart required).
    #[serde(default = "default_login_ip_long_window_secs")]
    pub login_ip_long_window_secs: u64,
    /// See [`Self::login_ip_long_window_secs`].
    #[serde(default = "default_login_ip_long_max_failures")]
    pub login_ip_long_max_failures: u32,
    /// See [`Self::login_ip_long_window_secs`].
    #[serde(default = "default_login_ip_long_base_ban_secs")]
    pub login_ip_long_base_ban_secs: u64,
    /// See [`Self::login_ip_long_window_secs`].
    #[serde(default = "default_login_ip_long_max_ban_secs")]
    pub login_ip_long_max_ban_secs: u64,
    /// Login throttle user-long daily bucket: per-account ceiling catching
    /// low-and-slow attacks, keyed on the claimed name even when unknown.
    /// Host-only (restart required).
    #[serde(default = "default_login_user_long_window_secs")]
    pub login_user_long_window_secs: u64,
    /// See [`Self::login_user_long_window_secs`].
    #[serde(default = "default_login_user_long_max_failures")]
    pub login_user_long_max_failures: u32,
    /// See [`Self::login_user_long_window_secs`].
    #[serde(default = "default_login_user_long_base_ban_secs")]
    pub login_user_long_base_ban_secs: u64,
    /// See [`Self::login_user_long_window_secs`].
    #[serde(default = "default_login_user_long_max_ban_secs")]
    pub login_user_long_max_ban_secs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_key: Option<String>,
    /// Path to the compiled frontend build directory (contains index.html and assets/).
    /// Not exposed via the JSON API; set via config.toml or `PICASU_WEB_ROOT` env var.
    #[serde(skip)]
    pub web_root: Option<PathBuf>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            address: "0.0.0.0".to_string(),
            port: 5673,
            data_home: None,
            image_home: None,
            upload_folder: default_upload_folder(),
            max_upload_size: default_max_upload_size(),
            read_only_mode: false,
            disable_img: false,
            fs_notify_watcher: true,
            normalize_upload_filenames: true,
            validate_upload_content: true,
            use_client_timestamp_info: false,
            allow_registration: false,
            trivial_passwords: false,
            bootstrap_admin: false,
            trust_proxy_forwarded_for: false,
            login_ip_window_secs: default_login_ip_window_secs(),
            login_ip_max_failures: default_login_ip_max_failures(),
            login_ip_base_ban_secs: default_login_ip_base_ban_secs(),
            login_ip_max_ban_secs: default_login_ip_max_ban_secs(),
            login_ip_long_window_secs: default_login_ip_long_window_secs(),
            login_ip_long_max_failures: default_login_ip_long_max_failures(),
            login_ip_long_base_ban_secs: default_login_ip_long_base_ban_secs(),
            login_ip_long_max_ban_secs: default_login_ip_long_max_ban_secs(),
            login_user_long_window_secs: default_login_user_long_window_secs(),
            login_user_long_max_failures: default_login_user_long_max_failures(),
            login_user_long_base_ban_secs: default_login_user_long_base_ban_secs(),
            login_user_long_max_ban_secs: default_login_user_long_max_ban_secs(),
            auth_key: None,
            web_root: None,
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
    #[serde(default)]
    pub(crate) trust_proxy_forwarded_for: bool,
    #[serde(default = "default_login_ip_window_secs")]
    pub(crate) login_ip_window_secs: u64,
    #[serde(default = "default_login_ip_max_failures")]
    pub(crate) login_ip_max_failures: u32,
    #[serde(default = "default_login_ip_base_ban_secs")]
    pub(crate) login_ip_base_ban_secs: u64,
    #[serde(default = "default_login_ip_max_ban_secs")]
    pub(crate) login_ip_max_ban_secs: u64,
    #[serde(default = "default_login_ip_long_window_secs")]
    pub(crate) login_ip_long_window_secs: u64,
    #[serde(default = "default_login_ip_long_max_failures")]
    pub(crate) login_ip_long_max_failures: u32,
    #[serde(default = "default_login_ip_long_base_ban_secs")]
    pub(crate) login_ip_long_base_ban_secs: u64,
    #[serde(default = "default_login_ip_long_max_ban_secs")]
    pub(crate) login_ip_long_max_ban_secs: u64,
    #[serde(default = "default_login_user_long_window_secs")]
    pub(crate) login_user_long_window_secs: u64,
    #[serde(default = "default_login_user_long_max_failures")]
    pub(crate) login_user_long_max_failures: u32,
    #[serde(default = "default_login_user_long_base_ban_secs")]
    pub(crate) login_user_long_base_ban_secs: u64,
    #[serde(default = "default_login_user_long_max_ban_secs")]
    pub(crate) login_user_long_max_ban_secs: u64,
}

impl Default for TomlServer {
    fn default() -> Self {
        Self {
            address: default_address(),
            port: default_port(),
            max_upload_size: default_max_upload_size(),
            trust_proxy_forwarded_for: false,
            login_ip_window_secs: default_login_ip_window_secs(),
            login_ip_max_failures: default_login_ip_max_failures(),
            login_ip_base_ban_secs: default_login_ip_base_ban_secs(),
            login_ip_max_ban_secs: default_login_ip_max_ban_secs(),
            login_ip_long_window_secs: default_login_ip_long_window_secs(),
            login_ip_long_max_failures: default_login_ip_long_max_failures(),
            login_ip_long_base_ban_secs: default_login_ip_long_base_ban_secs(),
            login_ip_long_max_ban_secs: default_login_ip_long_max_ban_secs(),
            login_user_long_window_secs: default_login_user_long_window_secs(),
            login_user_long_max_failures: default_login_user_long_max_failures(),
            login_user_long_base_ban_secs: default_login_user_long_base_ban_secs(),
            login_user_long_max_ban_secs: default_login_user_long_max_ban_secs(),
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
    pub(crate) image_home: Option<PathBuf>,
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
    #[serde(default)]
    pub(crate) allow_registration: bool,
    #[serde(default)]
    pub(crate) trivial_passwords: bool,
    #[serde(default)]
    pub(crate) bootstrap_admin: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) web_root: Option<PathBuf>,
}

impl Default for TomlGallery {
    fn default() -> Self {
        Self {
            data_home: None,
            image_home: None,
            upload_folder: default_upload_folder(),
            read_only_mode: false,
            disable_img: false,
            fs_notify_watcher: true,
            normalize_upload_filenames: true,
            validate_upload_content: true,
            use_client_timestamp_info: false,
            allow_registration: false,
            trivial_passwords: false,
            bootstrap_admin: false,
            web_root: None,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub(crate) struct TomlSecrets {
    // No fields: the signing key lives in the owner-only secrets file, and
    // legacy `[secrets] auth_key` entries are adopted at boot, never parsed
    // here. The section stays so old files keep parsing.
}

impl From<TomlFile> for AppConfig {
    fn from(t: TomlFile) -> Self {
        AppConfig {
            address: t.server.address,
            port: t.server.port,
            data_home: t.gallery.data_home,
            image_home: t.gallery.image_home,
            upload_folder: t.gallery.upload_folder,
            max_upload_size: t.server.max_upload_size,
            read_only_mode: t.gallery.read_only_mode,
            disable_img: t.gallery.disable_img,
            fs_notify_watcher: t.gallery.fs_notify_watcher,
            normalize_upload_filenames: t.gallery.normalize_upload_filenames,
            validate_upload_content: t.gallery.validate_upload_content,
            use_client_timestamp_info: t.gallery.use_client_timestamp_info,
            allow_registration: t.gallery.allow_registration,
            trivial_passwords: t.gallery.trivial_passwords,
            bootstrap_admin: t.gallery.bootstrap_admin,
            trust_proxy_forwarded_for: t.server.trust_proxy_forwarded_for,
            login_ip_window_secs: t.server.login_ip_window_secs,
            login_ip_max_failures: t.server.login_ip_max_failures,
            login_ip_base_ban_secs: t.server.login_ip_base_ban_secs,
            login_ip_max_ban_secs: t.server.login_ip_max_ban_secs,
            login_ip_long_window_secs: t.server.login_ip_long_window_secs,
            login_ip_long_max_failures: t.server.login_ip_long_max_failures,
            login_ip_long_base_ban_secs: t.server.login_ip_long_base_ban_secs,
            login_ip_long_max_ban_secs: t.server.login_ip_long_max_ban_secs,
            login_user_long_window_secs: t.server.login_user_long_window_secs,
            login_user_long_max_failures: t.server.login_user_long_max_failures,
            login_user_long_base_ban_secs: t.server.login_user_long_base_ban_secs,
            login_user_long_max_ban_secs: t.server.login_user_long_max_ban_secs,
            // The signing key never comes from the file mapping: boot
            // resolves it from the secrets file / legacy text / env.
            auth_key: None,
            web_root: t.gallery.web_root,
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
                trust_proxy_forwarded_for: c.trust_proxy_forwarded_for,
                login_ip_window_secs: c.login_ip_window_secs,
                login_ip_max_failures: c.login_ip_max_failures,
                login_ip_base_ban_secs: c.login_ip_base_ban_secs,
                login_ip_max_ban_secs: c.login_ip_max_ban_secs,
                login_ip_long_window_secs: c.login_ip_long_window_secs,
                login_ip_long_max_failures: c.login_ip_long_max_failures,
                login_ip_long_base_ban_secs: c.login_ip_long_base_ban_secs,
                login_ip_long_max_ban_secs: c.login_ip_long_max_ban_secs,
                login_user_long_window_secs: c.login_user_long_window_secs,
                login_user_long_max_failures: c.login_user_long_max_failures,
                login_user_long_base_ban_secs: c.login_user_long_base_ban_secs,
                login_user_long_max_ban_secs: c.login_user_long_max_ban_secs,
            },
            gallery: TomlGallery {
                data_home: c.data_home,
                image_home: c.image_home,
                upload_folder: c.upload_folder,
                read_only_mode: c.read_only_mode,
                disable_img: c.disable_img,
                fs_notify_watcher: c.fs_notify_watcher,
                normalize_upload_filenames: c.normalize_upload_filenames,
                validate_upload_content: c.validate_upload_content,
                use_client_timestamp_info: c.use_client_timestamp_info,
                allow_registration: c.allow_registration,
                trivial_passwords: c.trivial_passwords,
                bootstrap_admin: c.bootstrap_admin,
                web_root: c.web_root,
            },
            secrets: TomlSecrets {},
        }
    }
}

// ── Global config store ──────────────────────────────────────────────────────

pub static APP_CONFIG: OnceLock<RwLock<AppConfig>> = OnceLock::new();

/// Read-only accessor for the live server config: the single home of the
/// `RwLock` ceremony (the two `expect`s) so handlers never repeat it.
pub(crate) fn with_config<T>(f: impl FnOnce(&AppConfig) -> T) -> T {
    let guard = APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("lock poisoned");
    f(&guard)
}
/// The three login-throttle schedules with their knob names: `(names,
/// (window_secs, max_failures, base_ban_secs, max_ban_secs))` for
/// IP-short, IP-long, and user-long, in that order. Single assembly
/// point for validation and tracker construction, so a transposed field
/// fails in one place instead of compiling silently in two.
pub type ThrottleSchedules = [(&'static str, (u64, u32, u64, u64)); 3];

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

    /// Validate the three login-throttle bucket schedules against
    /// [`crate::auth::rate_limit::Bucket`]'s rules: no zero window, zero
    /// `max_failures`, zero base ban, or cap below base on any bucket.
    /// Called at load so a degenerate schedule fails startup instead of
    /// silently clamping to a policy nobody asked for.
    ///
    /// # Errors
    /// Names the offending bucket schedule when any tuple is degenerate.
    pub fn validate_login_throttle(&self) -> anyhow::Result<()> {
        use crate::auth::rate_limit::Bucket;
        for (knobs, (window, max_failures, base, cap)) in self.login_throttle_schedules() {
            Bucket::new(window, max_failures, base, cap).map_err(|e| {
                anyhow::anyhow!("invalid login throttle config [server] {knobs}: {e}")
            })?;
        }
        Ok(())
    }

    /// The three login-throttle schedules (see [`ThrottleSchedules`]).
    #[must_use]
    pub fn login_throttle_schedules(&self) -> ThrottleSchedules {
        [
            (
                "login_ip_window_secs/login_ip_max_failures/login_ip_base_ban_secs/login_ip_max_ban_secs",
                (
                    self.login_ip_window_secs,
                    self.login_ip_max_failures,
                    self.login_ip_base_ban_secs,
                    self.login_ip_max_ban_secs,
                ),
            ),
            (
                "login_ip_long_window_secs/login_ip_long_max_failures/login_ip_long_base_ban_secs/login_ip_long_max_ban_secs",
                (
                    self.login_ip_long_window_secs,
                    self.login_ip_long_max_failures,
                    self.login_ip_long_base_ban_secs,
                    self.login_ip_long_max_ban_secs,
                ),
            ),
            (
                "login_user_long_window_secs/login_user_long_max_failures/login_user_long_base_ban_secs/login_user_long_max_ban_secs",
                (
                    self.login_user_long_window_secs,
                    self.login_user_long_max_failures,
                    self.login_user_long_base_ban_secs,
                    self.login_user_long_max_ban_secs,
                ),
            ),
        ]
    }

    /// # Panics
    /// Panics if the global configuration is already initialized.
    /// # Errors
    /// Returns an error when the login-throttle configuration is
    /// degenerate; the caller must fail startup instead of serving with a
    /// silently altered throttle policy.
    pub fn init() -> anyhow::Result<()> {
        let config_path = get_config_path();
        let config_path_display = config_path.display();

        // First-launch: config.toml doesn't exist yet
        if !config_path.exists() {
            info!(
                "Configuration file not found at {config_path_display}. Creating default config.toml..."
            );

            let data_home =
                resolve_root("PICASU_DATA_HOME", "data", |p| p.data_dir().to_path_buf());

            let image_home = match std::env::var("PICASU_IMAGE_HOME") {
                Ok(p) => Some(PathBuf::from(p)),
                Err(_) => Some(data_home.join("images")),
            };

            let web_root = match std::env::var("PICASU_WEB_ROOT") {
                Ok(p) => Some(PathBuf::from(p)),
                Err(_) => Some(data_home.join("www")),
            };

            let config = AppConfig {
                data_home: Some(data_home),
                image_home,
                web_root,
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

        let raw_content = match fs::read_to_string(&config_path) {
            Ok(content) => content,
            Err(e) => {
                warn!("Failed to read config file {config_path_display}: {e}");
                String::new()
            }
        };
        if has_legacy_password_key(&raw_content) {
            warn!(
                "legacy single password is no longer supported and is ignored; enroll via --bootstrap-admin or the picasu-admin CLI"
            );
        }
        let mut config = match toml::from_str::<TomlFile>(&raw_content) {
            Ok(tf) => AppConfig::from(tf),
            Err(e) => {
                warn!("Failed to parse config at {config_path_display}: {e:?}, using defaults");
                AppConfig::default()
            }
        };

        // The signing key lives in the owner-only secrets file, never in
        // config.toml (a legacy `[secrets] auth_key` is ignored). Unset stays
        // unset for the per-process random fallback. `PICASU_AUTH_KEY` still
        // wins below without being persisted.
        let secrets_path = config
            .data_home
            .clone()
            .unwrap_or_else(|| get_data_path().clone())
            .join("auth")
            .join("secrets.json");
        config.auth_key = match crate::auth::secret_key::load_secret_key_from(&secrets_path) {
            Ok(key) => key,
            Err(e) => {
                warn!("secrets file unreadable, starting without a stored key: {e:#}");
                None
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

        // Degenerate throttle schedules fail startup (the caller maps this
        // to a non-zero exit) rather than clamp to a silent policy.
        config.validate_login_throttle()?;

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

        APP_CONFIG
            .set(RwLock::new(config))
            .expect("Config already initialized");

        // Let the logger know where we ended up
        info!("Configuration loaded successfully.");
        Ok(())
    }

    fn apply_env_overrides(config: &mut AppConfig) {
        Self::apply_env_overrides_with(config, |name| std::env::var(name).ok());
    }

    /// Env-override application with the variable lookup injected, so
    /// tests can drive precedence hermetically: the crate denies `unsafe`,
    /// hence no process-env mutation from tests.
    fn apply_env_overrides_with(config: &mut AppConfig, get: impl Fn(&str) -> Option<String>) {
        // Trimmed, non-empty string override into an `Option` field.
        macro_rules! set_str {
            ($field:ident, $var:literal) => {
                if let Some(val) = get($var) {
                    let p = val.trim().to_string();
                    if !p.is_empty() {
                        config.$field = Some(p.into());
                    }
                }
            };
        }
        // Parsed scalar override; an unparsable value keeps the current one.
        macro_rules! set_parse {
            ($field:ident, $var:literal) => {
                if let Some(val) = get($var)
                    && let Ok(parsed) = val.parse()
                {
                    config.$field = parsed;
                }
            };
        }
        set_str!(data_home, "PICASU_DATA_HOME");
        set_str!(image_home, "PICASU_IMAGE_HOME");
        set_parse!(port, "PICASU_PORT");
        if let Some(val) = get("PICASU_ADDRESS") {
            config.address = val;
        }
        set_parse!(read_only_mode, "PICASU_READ_ONLY_MODE");
        set_parse!(allow_registration, "PICASU_ALLOW_REGISTRATION");
        set_parse!(trivial_passwords, "PICASU_TRIVIAL_PASSWORDS");
        set_parse!(bootstrap_admin, "PICASU_BOOTSTRAP_ADMIN");
        set_parse!(
            trust_proxy_forwarded_for,
            "PICASU_TRUST_PROXY_FORWARDED_FOR"
        );
        set_parse!(login_ip_window_secs, "PICASU_LOGIN_IP_WINDOW_SECS");
        set_parse!(login_ip_max_failures, "PICASU_LOGIN_IP_MAX_FAILURES");
        set_parse!(login_ip_base_ban_secs, "PICASU_LOGIN_IP_BASE_BAN_SECS");
        set_parse!(login_ip_max_ban_secs, "PICASU_LOGIN_IP_MAX_BAN_SECS");
        set_parse!(
            login_ip_long_window_secs,
            "PICASU_LOGIN_IP_LONG_WINDOW_SECS"
        );
        set_parse!(
            login_ip_long_max_failures,
            "PICASU_LOGIN_IP_LONG_MAX_FAILURES"
        );
        set_parse!(
            login_ip_long_base_ban_secs,
            "PICASU_LOGIN_IP_LONG_BASE_BAN_SECS"
        );
        set_parse!(
            login_ip_long_max_ban_secs,
            "PICASU_LOGIN_IP_LONG_MAX_BAN_SECS"
        );
        set_parse!(
            login_user_long_window_secs,
            "PICASU_LOGIN_USER_LONG_WINDOW_SECS"
        );
        set_parse!(
            login_user_long_max_failures,
            "PICASU_LOGIN_USER_LONG_MAX_FAILURES"
        );
        set_parse!(
            login_user_long_base_ban_secs,
            "PICASU_LOGIN_USER_LONG_BASE_BAN_SECS"
        );
        set_parse!(
            login_user_long_max_ban_secs,
            "PICASU_LOGIN_USER_LONG_MAX_BAN_SECS"
        );
        set_parse!(disable_img, "PICASU_DISABLE_IMG");
        if let Some(val) = get("PICASU_FS_NOTIFY_WATCHER") {
            if let Ok(enabled) = val.parse() {
                config.fs_notify_watcher = enabled;
            } else {
                warn!("PICASU_FS_NOTIFY_WATCHER='{val}' is not 'true' or 'false', ignoring");
            }
        }
        if let Some(val) = get("PICASU_UPLOAD_FOLDER") {
            let trimmed = val.trim().to_string();
            if !trimmed.is_empty() {
                config.upload_folder = trimmed;
            }
        }
        if let Some(val) = get("PICASU_MAX_UPLOAD_SIZE") {
            let trimmed = val.trim().to_string();
            if !trimmed.is_empty() {
                config.max_upload_size = trimmed;
            }
        }
        if let Some(val) = get("PICASU_AUTH_KEY") {
            let trimmed = val.trim().to_string();
            config.auth_key = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            };
        }
        set_str!(web_root, "PICASU_WEB_ROOT");
    }

    /// # Panics
    /// Panics if the global configuration has not been initialized.
    /// # Errors
    /// Returns an error if the configuration cannot be serialized and saved to disk.
    pub fn update(mut new_config: AppConfig) -> anyhow::Result<()> {
        use crate::tasks::batcher::start_watcher::reload_watcher;

        info!("Updating configuration...");

        new_config.image_home = new_config.image_home.and_then(|p| {
            let cleaned = p.to_string_lossy().trim().trim_matches('"').to_string();
            if cleaned.is_empty() {
                None
            } else {
                Some(PathBuf::from(cleaned))
            }
        });

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

/// Whether the raw TOML text contains the removed `[secrets] password` key.
/// Old config files still parse (serde ignores the unknown key); the boot
/// path warns and continues. Pure so unit tests can exercise the detector
/// without a logger; the WARN itself is manual-verify in boot logs.
pub(crate) fn has_legacy_password_key(content: &str) -> bool {
    toml::from_str::<toml::Value>(content)
        .ok()
        .and_then(|v| v.get("secrets")?.get("password").cloned())
        .is_some_and(|p| p.as_str().is_some_and(|s| !s.is_empty()))
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
            image_home: Some(PathBuf::from("/tmp/images")),
            upload_folder: "test_uploads".to_string(),
            max_upload_size: "500MiB".to_string(),
            read_only_mode: true,
            disable_img: false,
            fs_notify_watcher: false,
            normalize_upload_filenames: false,
            validate_upload_content: true,
            use_client_timestamp_info: false,
            allow_registration: true,
            trivial_passwords: true,
            bootstrap_admin: true,
            trust_proxy_forwarded_for: true,
            login_ip_window_secs: 100,
            login_ip_max_failures: 4,
            login_ip_base_ban_secs: 20,
            login_ip_max_ban_secs: 200,
            login_ip_long_window_secs: 2000,
            login_ip_long_max_failures: 5,
            login_ip_long_base_ban_secs: 30,
            login_ip_long_max_ban_secs: 300,
            login_user_long_window_secs: 3000,
            login_user_long_max_failures: 6,
            login_user_long_base_ban_secs: 40,
            login_user_long_max_ban_secs: 400,
            auth_key: None,
            web_root: Some(PathBuf::from("/tmp/www")),
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
            image_home: Some(PathBuf::from("/images")),
            max_upload_size: "200MiB".to_string(),
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
        assert!(toml_str.contains("image_home = \"/images\""));
        assert!(
            !toml_str
                .lines()
                .any(|l| l.trim_start().starts_with("password =")),
            "no legacy password key may be written"
        );
        assert!(
            !toml_str.contains("auth_key"),
            "signing key lives in the secrets file, never config.toml"
        );
    }

    #[test]
    fn legacy_password_key_in_file_is_ignored() {
        // Old config files may still contain `[secrets] password = ...`.
        // Parsing must succeed and the password must have no effect: there
        // is no password field on the config anymore. The boot path warns
        // (see `has_legacy_password_key`, WARN is manual-verify in logs).
        let toml_str = r#"
[server]
port = 9999

[gallery]
read_only_mode = true

[secrets]
password = "hunter2"
auth_key = "jwt-secret"
"#;
        assert!(
            super::has_legacy_password_key(toml_str),
            "detector must see the legacy key"
        );
        let parsed: TomlFile = toml::from_str(toml_str).expect("old file must still parse");
        let config = AppConfig::from(parsed);
        assert_eq!(config.port, 9999);
        assert!(config.read_only_mode);
        // The file mapping carries no key: legacy keys are ignored.
        assert_eq!(config.auth_key, None);
    }

    #[test]
    fn no_legacy_password_key_absent() {
        let toml_str = r#"
[server]
port = 9999
"#;
        assert!(!super::has_legacy_password_key(toml_str));
    }

    #[test]
    fn allow_registration_defaults_to_false() {
        assert!(!AppConfig::default().allow_registration);
    }

    #[test]
    fn trivial_passwords_defaults_to_false() {
        assert!(!AppConfig::default().trivial_passwords);
    }

    #[test]
    fn bootstrap_admin_defaults_to_false() {
        assert!(!AppConfig::default().bootstrap_admin);
    }

    fn parse_app_config(toml_str: &str) -> AppConfig {
        let parsed: TomlFile = toml::from_str(toml_str).expect("failed to deserialize toml");
        AppConfig::from(parsed)
    }

    #[test]
    fn login_throttle_knobs_default_when_absent() {
        let config = parse_app_config("");
        assert!(!config.trust_proxy_forwarded_for);
        assert_eq!(config.login_ip_window_secs, 600);
        assert_eq!(config.login_ip_max_failures, 3);
        assert_eq!(config.login_ip_base_ban_secs, 10);
        assert_eq!(config.login_ip_max_ban_secs, 28_800);
        assert_eq!(config.login_ip_long_window_secs, 86_400);
        assert_eq!(config.login_ip_long_max_failures, 16);
        assert_eq!(config.login_ip_long_base_ban_secs, 1800);
        assert_eq!(config.login_ip_long_max_ban_secs, 28_800);
        assert_eq!(config.login_user_long_window_secs, 86_400);
        assert_eq!(config.login_user_long_max_failures, 18);
        assert_eq!(config.login_user_long_base_ban_secs, 1800);
        assert_eq!(config.login_user_long_max_ban_secs, 7200);
        assert!(
            config.validate_login_throttle().is_ok(),
            "the default schedules must validate"
        );
    }

    #[test]
    fn login_throttle_knobs_honor_toml_values() {
        let config = parse_app_config(
            r#"
[server]
trust_proxy_forwarded_for = true
login_ip_window_secs = 101
login_ip_max_failures = 4
login_ip_base_ban_secs = 7
login_ip_max_ban_secs = 90
login_ip_long_window_secs = 202
login_ip_long_max_failures = 5
login_ip_long_base_ban_secs = 11
login_ip_long_max_ban_secs = 91
login_user_long_window_secs = 303
login_user_long_max_failures = 6
login_user_long_base_ban_secs = 13
login_user_long_max_ban_secs = 92
"#,
        );
        assert!(config.trust_proxy_forwarded_for);
        assert_eq!(config.login_ip_window_secs, 101);
        assert_eq!(config.login_ip_max_failures, 4);
        assert_eq!(config.login_ip_base_ban_secs, 7);
        assert_eq!(config.login_ip_max_ban_secs, 90);
        assert_eq!(config.login_ip_long_window_secs, 202);
        assert_eq!(config.login_ip_long_max_failures, 5);
        assert_eq!(config.login_ip_long_base_ban_secs, 11);
        assert_eq!(config.login_ip_long_max_ban_secs, 91);
        assert_eq!(config.login_user_long_window_secs, 303);
        assert_eq!(config.login_user_long_max_failures, 6);
        assert_eq!(config.login_user_long_base_ban_secs, 13);
        assert_eq!(config.login_user_long_max_ban_secs, 92);

        // The values also survive a save/load round trip under [server].
        let tf = TomlFile::from(config);
        let toml_str = toml::to_string_pretty(&tf).expect("serialize toml");
        let server = toml_str
            .split("[gallery]")
            .next()
            .expect("server section first");
        assert!(server.contains("trust_proxy_forwarded_for = true"));
        assert!(server.contains("login_ip_window_secs = 101"));
        assert!(server.contains("login_user_long_max_ban_secs = 92"));
        let restored = parse_app_config(&toml_str);
        assert_eq!(restored.login_ip_long_window_secs, 202);
        assert!(restored.trust_proxy_forwarded_for);
    }

    #[test]
    fn login_throttle_env_overrides_win_over_toml() {
        // Fake lookup instead of process env (the crate denies `unsafe`,
        // hence no env mutation from tests): it also pins the exact
        // variable names — asking for anything else yields `None` and the
        // assertions below see the TOML values instead.
        let env: [(&str, &str); 13] = [
            ("PICASU_TRUST_PROXY_FORWARDED_FOR", "true"),
            ("PICASU_LOGIN_IP_WINDOW_SECS", "11"),
            ("PICASU_LOGIN_IP_MAX_FAILURES", "4"),
            ("PICASU_LOGIN_IP_BASE_BAN_SECS", "21"),
            ("PICASU_LOGIN_IP_MAX_BAN_SECS", "212"),
            ("PICASU_LOGIN_IP_LONG_WINDOW_SECS", "111"),
            ("PICASU_LOGIN_IP_LONG_MAX_FAILURES", "5"),
            ("PICASU_LOGIN_IP_LONG_BASE_BAN_SECS", "22"),
            ("PICASU_LOGIN_IP_LONG_MAX_BAN_SECS", "222"),
            ("PICASU_LOGIN_USER_LONG_WINDOW_SECS", "112"),
            ("PICASU_LOGIN_USER_LONG_MAX_FAILURES", "6"),
            ("PICASU_LOGIN_USER_LONG_BASE_BAN_SECS", "23"),
            ("PICASU_LOGIN_USER_LONG_MAX_BAN_SECS", "232"),
        ];

        // TOML deliberately carries different values: env must win.
        let mut config = parse_app_config(
            r#"
[server]
trust_proxy_forwarded_for = false
login_ip_window_secs = 999
login_ip_max_failures = 9
login_ip_base_ban_secs = 90
login_ip_max_ban_secs = 900
login_ip_long_window_secs = 998
login_ip_long_max_failures = 8
login_ip_long_base_ban_secs = 80
login_ip_long_max_ban_secs = 800
login_user_long_window_secs = 997
login_user_long_max_failures = 7
login_user_long_base_ban_secs = 70
login_user_long_max_ban_secs = 700
"#,
        );
        AppConfig::apply_env_overrides_with(&mut config, |name| {
            env.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_string())
        });

        assert!(config.trust_proxy_forwarded_for);
        assert_eq!(config.login_ip_window_secs, 11);
        assert_eq!(config.login_ip_max_failures, 4);
        assert_eq!(config.login_ip_base_ban_secs, 21);
        assert_eq!(config.login_ip_max_ban_secs, 212);
        assert_eq!(config.login_ip_long_window_secs, 111);
        assert_eq!(config.login_ip_long_max_failures, 5);
        assert_eq!(config.login_ip_long_base_ban_secs, 22);
        assert_eq!(config.login_ip_long_max_ban_secs, 222);
        assert_eq!(config.login_user_long_window_secs, 112);
        assert_eq!(config.login_user_long_max_failures, 6);
        assert_eq!(config.login_user_long_base_ban_secs, 23);
        assert_eq!(config.login_user_long_max_ban_secs, 232);
        assert!(config.validate_login_throttle().is_ok());
    }

    #[test]
    fn login_throttle_validation_rejects_degenerate_schedules() {
        // Zero window / zero max failures / zero base ban and a cap below
        // the base are rejected on each of the three buckets, never clamped.
        let cases: Vec<(&str, Box<dyn Fn() -> AppConfig>)> = vec![
            (
                "zero IP-short window",
                Box::new(|| AppConfig {
                    login_ip_window_secs: 0,
                    ..AppConfig::default()
                }),
            ),
            (
                "zero IP-short max failures",
                Box::new(|| AppConfig {
                    login_ip_max_failures: 0,
                    ..AppConfig::default()
                }),
            ),
            (
                "zero IP-short base ban",
                Box::new(|| AppConfig {
                    login_ip_base_ban_secs: 0,
                    ..AppConfig::default()
                }),
            ),
            (
                "IP-short cap below base",
                Box::new(|| AppConfig {
                    login_ip_base_ban_secs: 20,
                    login_ip_max_ban_secs: 10,
                    ..AppConfig::default()
                }),
            ),
            (
                "zero IP-long window",
                Box::new(|| AppConfig {
                    login_ip_long_window_secs: 0,
                    ..AppConfig::default()
                }),
            ),
            (
                "zero IP-long max failures",
                Box::new(|| AppConfig {
                    login_ip_long_max_failures: 0,
                    ..AppConfig::default()
                }),
            ),
            (
                "zero IP-long base ban",
                Box::new(|| AppConfig {
                    login_ip_long_base_ban_secs: 0,
                    ..AppConfig::default()
                }),
            ),
            (
                "IP-long cap below base",
                Box::new(|| AppConfig {
                    login_ip_long_base_ban_secs: 200,
                    login_ip_long_max_ban_secs: 100,
                    ..AppConfig::default()
                }),
            ),
            (
                "zero user-long window",
                Box::new(|| AppConfig {
                    login_user_long_window_secs: 0,
                    ..AppConfig::default()
                }),
            ),
            (
                "zero user-long max failures",
                Box::new(|| AppConfig {
                    login_user_long_max_failures: 0,
                    ..AppConfig::default()
                }),
            ),
            (
                "zero user-long base ban",
                Box::new(|| AppConfig {
                    login_user_long_base_ban_secs: 0,
                    ..AppConfig::default()
                }),
            ),
            (
                "user-long cap below base",
                Box::new(|| AppConfig {
                    login_user_long_base_ban_secs: 300,
                    login_user_long_max_ban_secs: 100,
                    ..AppConfig::default()
                }),
            ),
        ];
        for (label, make) in cases {
            let config = make();
            let err = config
                .validate_login_throttle()
                .expect_err(&format!("{label} must fail validation"));
            assert!(
                err.to_string().contains("login"),
                "error must name the login throttle: {err}"
            );
        }

        // Degenerate-but-valid edges still load.
        let minimal = AppConfig {
            login_ip_window_secs: 1,
            login_ip_max_failures: 1,
            login_ip_base_ban_secs: 1,
            login_ip_max_ban_secs: 1,
            login_ip_long_window_secs: 1,
            login_ip_long_max_failures: 1,
            login_ip_long_base_ban_secs: 1,
            login_ip_long_max_ban_secs: 1,
            login_user_long_window_secs: 1,
            login_user_long_max_failures: 1,
            login_user_long_base_ban_secs: 1,
            login_user_long_max_ban_secs: 1,
            ..AppConfig::default()
        };
        minimal.validate_login_throttle().expect("minimal schedule");
        let cap_eq_base = AppConfig {
            login_ip_base_ban_secs: 10,
            login_ip_max_ban_secs: 10,
            ..AppConfig::default()
        };
        cap_eq_base
            .validate_login_throttle()
            .expect("cap equal to base is valid");
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
        assert!(!config.allow_registration);
        assert!(!config.bootstrap_admin);
    }
}
