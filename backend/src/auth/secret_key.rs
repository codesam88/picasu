use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// On-disk shape of the secrets file: the JWT signing key only.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecretsDoc {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    auth_key: Option<String>,
}

/// Path of the JSON secrets file: `<DATA_HOME>/auth/secrets.json`.
/// Holds the JWT signing key outside `config.toml` so the config file
/// carries no secret material.
#[must_use]
pub fn secrets_file_path() -> std::path::PathBuf {
    crate::storage::files::get_data_path()
        .join("auth")
        .join("secrets.json")
}

/// Load the stored signing key; a missing file loads as `None`, an empty
/// key reads as absent. A present-but-corrupt file is an error.
///
/// # Errors
/// Returns an error if the file exists but cannot be read or parsed.
pub fn load_secret_key_from(path: &std::path::Path) -> Result<Option<String>> {
    let bytes = match std::fs::read(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(e).context(format!("failed to read secrets file {}", path.display()));
        }
        Ok(bytes) => bytes,
    };
    let doc = serde_json::from_slice::<SecretsDoc>(&bytes)
        .context(format!("failed to parse secrets file {}", path.display()))?;
    Ok(doc.auth_key.filter(|k| !k.trim().is_empty()))
}

/// Persist `key` atomically (tmp-file + rename) with `0600` permissions,
/// creating the parent directory on first use.
///
/// # Errors
/// Returns an error if the directory or file cannot be written.
pub fn store_secret_key_to(path: &std::path::Path, key: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .context(format!("failed to create secrets dir {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(&SecretsDoc {
        auth_key: Some(key.to_string()),
    })
    .context("failed to serialize secrets file")?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &bytes)
        .context(format!("failed to write secrets file {}", tmp.display()))?;
    super::password::restrict_permissions(&tmp);
    std::fs::rename(&tmp, path).context(format!(
        "failed to move secrets file {} into place",
        path.display()
    ))?;
    Ok(())
}

/// Load the signing key from the production secrets file.
///
/// # Errors
/// Returns an error if the file exists but cannot be read or parsed.
pub fn load_secret_key() -> Result<Option<String>> {
    load_secret_key_from(&secrets_file_path())
}

/// Persist `key` to the production secrets file.
///
/// # Errors
/// Returns an error if the directory or file cannot be written.
pub fn store_secret_key(key: &str) -> Result<()> {
    store_secret_key_to(&secrets_file_path(), key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(name);
        (dir, path)
    }

    #[test]
    fn missing_file_loads_as_none() {
        let (_dir, path) = tmp_path("secrets.json");
        assert_eq!(
            load_secret_key_from(&path).expect("load"),
            None,
            "absent secrets file means no stored key"
        );
    }

    #[test]
    fn stored_key_round_trips() {
        let (_dir, path) = tmp_path("secrets.json");
        store_secret_key_to(&path, "test-signing-key").expect("store");
        assert_eq!(
            load_secret_key_from(&path).expect("load"),
            Some("test-signing-key".to_string())
        );
    }

    #[test]
    fn corrupt_file_is_an_error() {
        let (_dir, path) = tmp_path("secrets.json");
        std::fs::write(&path, b"{not json").expect("write corrupt");
        assert!(
            load_secret_key_from(&path).is_err(),
            "corrupt secrets file must error, not read as absent"
        );
    }

    #[test]
    fn empty_key_reads_as_absent() {
        let (_dir, path) = tmp_path("secrets.json");
        std::fs::write(&path, br#"{"authKey": ""}"#).expect("write empty");
        assert_eq!(
            load_secret_key_from(&path).expect("load"),
            None,
            "blank key normalizes to absent like the config path does"
        );
    }

    #[cfg(unix)]
    #[test]
    fn stored_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let (_dir, path) = tmp_path("secrets.json");
        store_secret_key_to(&path, "test-signing-key").expect("store");
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "secrets file must be owner-only, got {mode:o}");
    }
}
