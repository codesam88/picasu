//! Location of the user store for the offline administration CLI.
//!
//! The store is a single file, `<data home>/auth/users.redb`. The server
//! derives its data home from the environment and its config file; the CLI
//! resolves the same file the same way, plus an explicit path for the cases
//! where the admin knows better.

use std::path::{Path, PathBuf};

/// File name of the user store inside the data home's `auth/` directory.
pub const USER_STORE_FILE_NAME: &str = "users.redb";

/// Where a resolved user store location came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserStoreSource {
    /// An explicit path given on the command line (`--userdb`).
    Explicit,
    /// The `PICASU_DATA_HOME` environment variable.
    DataHomeEnv,
    /// The `data_home` key of the server's `config.toml`.
    ConfigFile,
}

/// A resolved user store location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserStoreLocation {
    /// The store file the CLI operates on.
    pub path: PathBuf,
    /// Where the location came from.
    pub source: UserStoreSource,
    /// The data home, when one is known. `user del` needs it to reach the
    /// session database; with an explicit store path alone it is unknown.
    pub data_home: Option<PathBuf>,
}

/// The user store file inside `data_home`.
#[must_use]
pub fn user_store_path_in(data_home: &Path) -> PathBuf {
    data_home.join("auth").join(USER_STORE_FILE_NAME)
}

/// Resolve the user store location.
///
/// Precedence: an explicit path, then `PICASU_DATA_HOME`, then the config
/// file's `data_home`. Returns `None` when no location is known — the CLI
/// must not guess one, and in particular must not create a store in a
/// location nobody chose.
#[must_use]
pub fn resolve_user_store(
    explicit: Option<&Path>,
    env_data_home: Option<&Path>,
    config_data_home: Option<&Path>,
) -> Option<UserStoreLocation> {
    let data_home = env_data_home.or(config_data_home);
    if let Some(explicit) = explicit {
        return Some(UserStoreLocation {
            path: explicit.to_path_buf(),
            source: UserStoreSource::Explicit,
            data_home: data_home.map(Path::to_path_buf),
        });
    }
    let (home, source) = match (env_data_home, config_data_home) {
        (Some(home), _) => (home, UserStoreSource::DataHomeEnv),
        (None, Some(home)) => (home, UserStoreSource::ConfigFile),
        (None, None) => return None,
    };
    Some(UserStoreLocation {
        path: user_store_path_in(home),
        source,
        data_home: Some(home.to_path_buf()),
    })
}

/// Resolve the user store location from the process environment: the
/// `PICASU_DATA_HOME` variable and the server's `config.toml`.
#[must_use]
pub fn locate_user_store(explicit: Option<&Path>) -> Option<UserStoreLocation> {
    let env_data_home = std::env::var("PICASU_DATA_HOME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let config_data_home = crate::model::config::configured_data_home();
    resolve_user_store(
        explicit,
        env_data_home.as_deref(),
        config_data_home.as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn explicit_path_wins_over_every_other_source() {
        let location = resolve_user_store(
            Some(&path("/explicit/users.redb")),
            Some(&path("/env")),
            Some(&path("/config")),
        )
        .expect("explicit path must resolve");
        assert_eq!(location.path, path("/explicit/users.redb"));
        assert_eq!(location.source, UserStoreSource::Explicit);
        assert_eq!(location.data_home, Some(path("/env")));
    }

    #[test]
    fn explicit_path_without_a_data_home_keeps_the_data_home_unknown() {
        let location = resolve_user_store(Some(&path("/explicit/users.redb")), None, None)
            .expect("explicit path must resolve");
        assert_eq!(location.path, path("/explicit/users.redb"));
        assert_eq!(location.source, UserStoreSource::Explicit);
        assert_eq!(location.data_home, None);
    }

    #[test]
    fn env_data_home_wins_over_the_config_file() {
        let location = resolve_user_store(None, Some(&path("/env")), Some(&path("/config")))
            .expect("env must resolve");
        assert_eq!(location.path, path("/env/auth/users.redb"));
        assert_eq!(location.source, UserStoreSource::DataHomeEnv);
        assert_eq!(location.data_home, Some(path("/env")));
    }

    #[test]
    fn config_file_data_home_is_used_when_the_env_is_unset() {
        let location =
            resolve_user_store(None, None, Some(&path("/config"))).expect("config must resolve");
        assert_eq!(location.path, path("/config/auth/users.redb"));
        assert_eq!(location.source, UserStoreSource::ConfigFile);
        assert_eq!(location.data_home, Some(path("/config")));
    }

    #[test]
    fn without_any_source_no_location_is_known() {
        assert_eq!(resolve_user_store(None, None, None), None);
    }

    #[test]
    fn the_store_file_name_is_stable() {
        assert_eq!(USER_STORE_FILE_NAME, "users.redb");
        assert_eq!(
            user_store_path_in(&path("/data")),
            path("/data/auth/users.redb")
        );
    }
}
