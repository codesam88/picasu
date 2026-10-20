use directories::ProjectDirs;
use log::{error, info};
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

pub static DATA_PATH: OnceLock<PathBuf> = OnceLock::new();
pub static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

const CONFIG_FILE_NAME: &str = "config.toml";

/// Where a root directory comes from, before it is created.
enum RootSource {
    /// The environment override.
    Env(PathBuf),
    /// The legacy single-folder layout: the working directory.
    LegacyCwd,
    /// The OS-standard directory.
    OsStandard(PathBuf),
    /// No OS-standard directory could be determined.
    Undetermined,
}

/// Resolve the source of a root directory. Shared by [`resolve_root`] (which
/// creates the directory) and [`resolve_root_path`] (which does not), so the
/// precedence cannot drift between them.
fn resolve_root_source(env_var: &str, os_dir: impl FnOnce(&ProjectDirs) -> PathBuf) -> RootSource {
    if let Ok(p) = std::env::var(env_var) {
        return RootSource::Env(PathBuf::from(p));
    }

    // Legacy back-compat: every pre-existing install has `config.toml`
    // sitting next to its `db`/`object`/`upload` folders in the working
    // directory. Detecting that file is a more precise signal than the old
    // "does ./db or ./object exist" sniff, and covers both roots with one
    // check.
    if Path::new(CONFIG_FILE_NAME).exists() {
        return RootSource::LegacyCwd;
    }

    if let Some(proj_dirs) = ProjectDirs::from("com", "picasu", "picasu") {
        return RootSource::OsStandard(os_dir(&proj_dirs));
    }

    RootSource::Undetermined
}

/// Resolves a root directory without touching the filesystem: environment
/// override > legacy single-folder layout > OS-standard directory >
/// working directory. Nothing is created and nothing is logged.
pub fn resolve_root_path(env_var: &str, os_dir: impl FnOnce(&ProjectDirs) -> PathBuf) -> PathBuf {
    match resolve_root_source(env_var, os_dir) {
        RootSource::Env(dir) | RootSource::OsStandard(dir) => dir,
        RootSource::LegacyCwd | RootSource::Undetermined => PathBuf::from("."),
    }
}

/// Resolves a root directory using a fixed precedence:
/// 1. `env_var`, if set — explicit override (e.g. `PICASU_DATA_HOME`).
/// 2. The legacy single-folder layout: if `./config.toml` already exists in
///    the working directory, pre-dates the split between config/data
///    locations below and must keep working unchanged.
/// 3. The OS-standard directory for `kind` (XDG on Linux, equivalent
///    conventions on Windows/macOS via the `directories` crate — which
///    itself honors `$XDG_CONFIG_HOME`/`$XDG_DATA_HOME` if set).
/// 4. The working directory, as a last resort if the OS directory can't be
///    determined at all.
pub fn resolve_root(
    env_var: &str,
    kind: &str,
    os_dir: impl FnOnce(&ProjectDirs) -> PathBuf,
) -> PathBuf {
    match resolve_root_source(env_var, os_dir) {
        RootSource::Env(dir) => {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                error!(
                    "Failed to create {env_var} directory {}: {e}",
                    dir.display()
                );
            }
            info!(
                "{env_var} override detected. Using {kind} directory: {}",
                dir.display()
            );
            dir
        }
        RootSource::LegacyCwd => {
            info!("Legacy single-folder layout detected — using cwd for {kind}");
            PathBuf::from(".")
        }
        RootSource::OsStandard(dir) => {
            if !dir.exists()
                && let Err(e) = std::fs::create_dir_all(&dir)
            {
                error!("Failed to create {kind} directory {}: {e}", dir.display());
                return PathBuf::from(".");
            }
            info!("Using OS-standard {kind} directory: {}", dir.display());
            dir
        }
        RootSource::Undetermined => {
            info!("Could not determine system {kind} directory. Defaulting to cwd.");
            PathBuf::from(".")
        }
    }
}

/// Returns the path to `config.toml`. See [`get_config_dir`] for how the
/// containing directory is resolved.
pub fn get_config_path() -> PathBuf {
    get_config_dir().join(CONFIG_FILE_NAME)
}

/// Directory holding `config.toml`. Override with `PICASU_CONFIG_HOME`;
/// otherwise resolved independently of [`get_data_path`] (see [`resolve_root`]).
pub fn get_config_dir() -> &'static PathBuf {
    CONFIG_DIR.get_or_init(|| {
        resolve_root("PICASU_CONFIG_HOME", "config", |p| {
            p.config_dir().to_path_buf()
        })
    })
}

/// Path of `config.toml`, resolved like [`get_config_path`] but without
/// creating directories: the offline CLI reads the file and must leave no
/// trace behind.
pub fn config_path_readonly() -> PathBuf {
    resolve_root_path("PICASU_CONFIG_HOME", |p| p.config_dir().to_path_buf()).join(CONFIG_FILE_NAME)
}

/// Directory holding `db/` and `object/compressed/` — derived index and
/// thumbnail/preview cache only. Override with `PICASU_DATA_HOME`.
/// Originals are never duplicated here; `IMAGE_HOME` is the single copy
/// (see `TODO.md` "Storage architecture fix"). This is still real,
/// back-up-worthy data — `db/index_v5.redb` is the only store of record for
/// tags/album-assignments/flags. (A handful of files under `db/` —
/// `cache_db.redb`, `temp_db.redb`, `expire_db.redb` — *are* safely
/// disposable; splitting those into a dedicated `PICASU_STATE_HOME` is a
/// possible follow-up, not yet done.)
pub fn get_data_path() -> &'static PathBuf {
    DATA_PATH
        .get_or_init(|| resolve_root("PICASU_DATA_HOME", "data", |p| p.data_dir().to_path_buf()))
}

/// Get the image path from the current config.
pub fn get_resolved_image_home() -> Option<std::path::PathBuf> {
    crate::model::config::with_config(|c| c.image_home.clone())
}
