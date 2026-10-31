use directories::ProjectDirs;
use log::{error, info};
use std::path::PathBuf;
use std::path::{Component, Path};
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

/// Resolved personal-users root, or `None` when the feature is disabled
/// (`users_home` unset/empty).
// Not yet called by request handlers; S1 wires indexing/watcher.
#[allow(dead_code)]
pub fn get_resolved_users_home() -> Option<std::path::PathBuf> {
    crate::model::config::with_config(|c| c.users_home.clone())
}

/// Classification of a canonical path relative to the configured roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathClassification {
    Public,
    Personal { owner: String },
    UsersRoot,
    Outside,
}

fn clean_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn canonicalize_if_exists(p: &Path) -> PathBuf {
    if let Ok(c) = std::fs::canonicalize(p) {
        return c;
    }
    // Canonicalize the longest existing prefix so symlinked roots resolve
    // even when the final component (file/dir) does not yet exist.
    let mut prefix = p.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    while !prefix.as_os_str().is_empty() && !prefix.exists() {
        if let Some(name) = prefix.file_name() {
            tail.push(name.to_os_string());
            prefix = prefix.parent().map(Path::to_path_buf).unwrap_or_default();
        } else {
            break;
        }
    }
    let mut out = std::fs::canonicalize(&prefix).unwrap_or_else(|_| clean_path(&prefix));
    for comp in tail.into_iter().rev() {
        out.push(comp);
    }
    if out.as_os_str().is_empty() {
        clean_path(p)
    } else {
        out
    }
}

fn is_safe_segment(s: &str) -> bool {
    use unicode_normalization::UnicodeNormalization;
    if s.is_empty() || s == "." || s == ".." {
        return false;
    }
    let normalized: String = s.nfkc().collect();
    if normalized.contains('/') || normalized.contains('\\') {
        return false;
    }
    if normalized == "." || normalized == ".." {
        return false;
    }
    let p = Path::new(s);
    let mut comps = p.components();
    match (comps.next(), comps.next()) {
        (Some(Component::Normal(_)), None) => {}
        _ => return false,
    }
    if s.chars().any(char::is_control) {
        return false;
    }
    // Reject known separator homoglyphs that NFKC may not collapse.
    for c in s.chars() {
        match c {
            '/' | '\\' | '／' | '＼' | '∕' | '⁄' | '﹨' => return false,
            _ => {}
        }
    }
    true
}

/// Why [`users_dir_for`] could not resolve a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsersDirError {
    /// Personal areas are disabled (`users_home` unset/empty).
    Disabled,
    /// The user id is not a single safe path segment.
    InvalidUser,
}

/// Resolve the on-disk directory owned by `user_id`.
// Not yet called by request handlers; S1 wires indexing/watcher.
#[allow(dead_code)]
pub fn users_dir_for(user_id: &str) -> Result<PathBuf, UsersDirError> {
    let root = get_resolved_users_home().ok_or(UsersDirError::Disabled)?;
    let canonical =
        crate::auth::users::validate_user_id(user_id).map_err(|_| UsersDirError::InvalidUser)?;
    if !is_safe_segment(&canonical) {
        return Err(UsersDirError::InvalidUser);
    }
    Ok(root.join(canonical))
}

/// Classify a canonical path as public, personal (with owner), the users
/// root itself, or outside both roots.
// Not yet called by request handlers; S1 wires indexing/watcher.
#[allow(dead_code)]
pub fn classify_path(canonical: &Path) -> PathClassification {
    let canonical = canonicalize_if_exists(canonical);

    if let Some(users_root) = get_resolved_users_home() {
        let users_root = canonicalize_if_exists(&users_root);
        if canonical == users_root {
            return PathClassification::UsersRoot;
        }
        if let Ok(rel) = canonical.strip_prefix(&users_root) {
            let mut comps = rel.components();
            if let Some(Component::Normal(seg)) = comps.next() {
                return PathClassification::Personal {
                    owner: seg.to_string_lossy().to_string(),
                };
            }
            return PathClassification::UsersRoot;
        }
    }

    if let Some(image_root) = get_resolved_image_home() {
        let image_root = canonicalize_if_exists(&image_root);
        if canonical == image_root || canonical.starts_with(&image_root) {
            return PathClassification::Public;
        }
    }

    PathClassification::Outside
}

/// Validate that the two roots do not equal or nest in either direction.
// Not yet called by request handlers; S1 wires indexing/watcher.
#[allow(dead_code)]
pub fn validate_path_roots<I, U>(image_home: Option<I>, users_home: Option<U>) -> anyhow::Result<()>
where
    I: AsRef<Path>,
    U: AsRef<Path>,
{
    if let (Some(img), Some(usr)) = (image_home, users_home) {
        let img = canonicalize_if_exists(img.as_ref());
        let usr = canonicalize_if_exists(usr.as_ref());
        if img == usr || img.starts_with(&usr) || usr.starts_with(&img) {
            return Err(anyhow::anyhow!(
                "users_home must not equal or be nested inside/outside image_home"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::config::AppConfig;
    use std::path::PathBuf;

    #[test]
    fn validate_path_roots_rejects_equal_and_nested() {
        let img = PathBuf::from("/tmp/img");
        let usr = PathBuf::from("/tmp/img");
        assert!(validate_path_roots(Some(&img), Some(&usr)).is_err());

        let usr = PathBuf::from("/tmp/img/sub");
        assert!(validate_path_roots(Some(&img), Some(&usr)).is_err());

        let img = PathBuf::from("/tmp/img/sub");
        let usr = PathBuf::from("/tmp/img");
        assert!(validate_path_roots(Some(&img), Some(&usr)).is_err());

        let img = PathBuf::from("/tmp/img");
        let usr = PathBuf::from("/tmp/users");
        assert!(validate_path_roots(Some(&img), Some(&usr)).is_ok());

        assert!(validate_path_roots(None::<PathBuf>, Some(PathBuf::from("/tmp/users"))).is_ok());
    }

    #[test]
    fn validate_path_roots_rejects_symlinked_nested_root() {
        let tmp = std::env::temp_dir().join("picasu-s0-symlink-validate");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).expect("tmp");
        let image = tmp.join("image");
        let users = tmp.join("users");
        std::fs::create_dir_all(&image).expect("image");
        std::fs::create_dir_all(&users).expect("users");
        let link = tmp.join("link-to-image");
        let _ = std::fs::remove_file(&link);
        if std::os::unix::fs::symlink(&image, &link).is_err() {
            let _ = std::fs::remove_dir_all(&tmp);
            return;
        }
        // users_home symlinked into image_home must be rejected
        assert!(validate_path_roots(Some(&image), Some(&link)).is_err());
        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn appconfig_validate_users_home() {
        let cfg = AppConfig {
            image_home: Some(PathBuf::from("/tmp/img")),
            users_home: Some(PathBuf::from("/tmp/img")),
            ..AppConfig::default()
        };
        assert!(cfg.validate_users_home().is_err());

        let cfg = AppConfig {
            image_home: Some(PathBuf::from("/tmp/img")),
            users_home: Some(PathBuf::from("/tmp/users")),
            ..AppConfig::default()
        };
        assert!(cfg.validate_users_home().is_ok());
    }

    #[test]
    fn users_dir_for_disabled_and_invalid() {
        let _g = crate::tests::bootstrap::lock();
        let _ = &*crate::tests::bootstrap::TEST_ENV;
        {
            let mut c = crate::model::config::APP_CONFIG
                .get()
                .expect("cfg")
                .write()
                .expect("lock");
            c.users_home = None;
        }
        assert_eq!(users_dir_for("alice"), Err(UsersDirError::Disabled));

        let root = tempfile::tempdir().expect("tmp");
        let users = root.path().join("users");
        std::fs::create_dir_all(&users).expect("users");
        {
            let mut c = crate::model::config::APP_CONFIG
                .get()
                .expect("cfg")
                .write()
                .expect("lock");
            c.image_home = Some(root.path().join("images"));
            c.users_home = Some(users.clone());
        }
        assert_eq!(users_dir_for(""), Err(UsersDirError::InvalidUser));
        assert_eq!(users_dir_for("a/b"), Err(UsersDirError::InvalidUser));
        assert_eq!(users_dir_for("alice").expect("ok"), users.join("alice"));

        {
            let mut c = crate::model::config::APP_CONFIG
                .get()
                .expect("cfg")
                .write()
                .expect("lock");
            c.users_home = None;
        }
    }

    #[test]
    fn classify_path_cases() {
        let _g = crate::tests::bootstrap::lock();
        let _ = &*crate::tests::bootstrap::TEST_ENV;
        let root = tempfile::tempdir().expect("tmp");
        let images = root.path().join("images");
        let users = root.path().join("users");
        std::fs::create_dir_all(&images).expect("images");
        std::fs::create_dir_all(&users).expect("users");
        {
            let mut c = crate::model::config::APP_CONFIG
                .get()
                .expect("cfg")
                .write()
                .expect("lock");
            c.image_home = Some(images.clone());
            c.users_home = Some(users.clone());
        }

        assert_eq!(
            classify_path(&images.join("a.jpg")),
            PathClassification::Public
        );
        assert_eq!(
            classify_path(&users.join("alice").join("x.jpg")),
            PathClassification::Personal {
                owner: "alice".to_string()
            }
        );
        assert_eq!(classify_path(&users), PathClassification::UsersRoot);
        assert_eq!(
            classify_path(&root.path().join("other").join("z.jpg")),
            PathClassification::Outside
        );

        // classify through a symlinked users_home still yields Personal
        let link = root.path().join("users-link");
        let _ = std::fs::remove_file(&link);
        if std::os::unix::fs::symlink(&users, &link).is_ok() {
            // point config at the symlinked root temporarily
            {
                let mut c = crate::model::config::APP_CONFIG
                    .get()
                    .expect("cfg")
                    .write()
                    .expect("lock");
                c.users_home = Some(link.clone());
            }
            assert_eq!(
                classify_path(&link.join("alice").join("x.jpg")),
                PathClassification::Personal {
                    owner: "alice".to_string()
                }
            );
            {
                let mut c = crate::model::config::APP_CONFIG
                    .get()
                    .expect("cfg")
                    .write()
                    .expect("lock");
                c.users_home = Some(users.clone());
            }
            let _ = std::fs::remove_file(&link);
        }

        {
            let mut c = crate::model::config::APP_CONFIG
                .get()
                .expect("cfg")
                .write()
                .expect("lock");
            c.users_home = None;
        }
    }
}
