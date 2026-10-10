use anyhow::Context as _;
use pbkdf2::pbkdf2_hmac;
use rand::{TryRng, rngs::SysRng};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;

/// PBKDF2-HMAC-SHA256 iteration count for new password hashes.
pub const DEFAULT_ITERATIONS: u32 = 600_000;
/// Length of the per-password random salt in bytes.
pub const SALT_LEN: usize = 16;
/// Length of the derived key in bytes.
pub const HASH_LEN: usize = 32;

/// A salted password hash. Salt and hash serialize as base64 strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasswordHash {
    #[serde(with = "base64_salt")]
    pub salt: [u8; SALT_LEN],
    #[serde(with = "base64_hash")]
    pub hash: [u8; HASH_LEN],
    pub iterations: u32,
}

/// Hash a password with a fresh random salt.
///
/// # Panics
/// Panics if the system RNG fails, which indicates a broken host.
#[must_use]
pub fn hash_password(password: &str) -> PasswordHash {
    let mut salt = [0u8; SALT_LEN];
    SysRng
        .try_fill_bytes(&mut salt)
        .expect("system RNG failure while generating password salt");
    let hash = derive(password, &salt, DEFAULT_ITERATIONS);
    PasswordHash {
        salt,
        hash,
        iterations: DEFAULT_ITERATIONS,
    }
}

/// Check a password against a stored hash using a constant-time comparison.
#[must_use]
pub fn verify_password(password: &str, expected: &PasswordHash) -> bool {
    let candidate = derive(password, &expected.salt, expected.iterations);
    candidate.ct_eq(&expected.hash).into()
}

fn derive(password: &str, salt: &[u8], iterations: u32) -> [u8; HASH_LEN] {
    let mut out = [0u8; HASH_LEN];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, iterations, &mut out);
    out
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PasswdDoc {
    #[serde(default)]
    users: std::collections::HashMap<String, PasswordHash>,
}

/// Borrowed view of [`PasswdDoc`] for serialization without cloning.
#[derive(Debug, Serialize)]
struct PasswdDocRef<'a> {
    users: &'a std::collections::HashMap<String, PasswordHash>,
}

/// JSON-backed password store at an explicit file path.
///
/// On-disk shape is `{"users": {id: {salt, hash, iterations}}}` with
/// base64-encoded salt and hash. Writes are atomic (tmp-file + rename) with
/// best-effort `0600` permissions on unix.
#[derive(Debug)]
pub struct PasswdFile {
    path: std::path::PathBuf,
    users: std::collections::HashMap<String, PasswordHash>,
}

impl PasswdFile {
    /// Load the store from `path`. A missing file loads as an empty store;
    /// a corrupt file returns an error.
    ///
    /// # Errors
    /// Returns an error if the file exists but cannot be read or parsed.
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let users = match std::fs::read(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::collections::HashMap::new(),
            Err(e) => {
                return Err(e).context(format!("failed to read passwd file {}", path.display()));
            }
            Ok(bytes) => {
                serde_json::from_slice::<PasswdDoc>(&bytes)
                    .context(format!("failed to parse passwd file {}", path.display()))?
                    .users
            }
        };
        Ok(Self {
            path: path.to_path_buf(),
            users,
        })
    }

    /// Persist the store atomically via tmp-file + rename.
    ///
    /// # Errors
    /// Returns an error if the file cannot be written.
    pub fn save(&self) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec_pretty(&PasswdDocRef { users: &self.users })
            .context("failed to serialize passwd file")?;
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, &bytes)
            .context(format!("failed to write passwd file {}", tmp.display()))?;
        restrict_permissions(&tmp);
        std::fs::rename(&tmp, &self.path).context(format!(
            "failed to move passwd file {} into place",
            self.path.display()
        ))?;
        Ok(())
    }

    /// Hash `password` for `user_id` (replacing any existing entry) and persist.
    ///
    /// # Errors
    /// Returns an error if the store cannot be persisted.
    pub fn set_password(&mut self, user_id: &str, password: &str) -> anyhow::Result<()> {
        self.users
            .insert(user_id.to_string(), hash_password(password));
        self.save()
    }

    /// Check `password` for `user_id`. Unknown users fail closed.
    #[must_use]
    pub fn verify(&self, user_id: &str, password: &str) -> bool {
        self.users
            .get(user_id)
            .is_some_and(|stored| verify_password(password, stored))
    }

    /// Whether the store holds no users.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.users.is_empty()
    }
}

/// Best-effort `0600` on the passwd file; permission errors are ignored and
/// non-unix platforms are a no-op.
fn restrict_permissions(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

macro_rules! base64_field {
    ($name:ident, $len:expr) => {
        mod $name {
            use base64::{Engine as _, engine::general_purpose};
            use serde::de::Error as _;
            use serde::{Deserialize, Deserializer, Serializer};

            pub fn serialize<S: Serializer>(
                bytes: &[u8; $len],
                serializer: S,
            ) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&general_purpose::STANDARD.encode(bytes))
            }

            pub fn deserialize<'de, D: Deserializer<'de>>(
                deserializer: D,
            ) -> Result<[u8; $len], D::Error> {
                let text = String::deserialize(deserializer)?;
                let raw = general_purpose::STANDARD
                    .decode(text.as_str())
                    .map_err(D::Error::custom)?;
                raw.try_into()
                    .map_err(|_| D::Error::custom("invalid byte length"))
            }
        }
    };
}

base64_field!(base64_salt, super::SALT_LEN);
base64_field!(base64_hash, super::HASH_LEN);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_verify_roundtrip() {
        let stored = hash_password("correct horse");
        assert!(verify_password("correct horse", &stored));
    }

    #[test]
    fn wrong_password_rejected() {
        let stored = hash_password("correct horse");
        assert!(!verify_password("wrong password", &stored));
    }

    #[test]
    fn tampered_salt_rejected() {
        let mut stored = hash_password("correct horse");
        stored.salt[0] ^= 0xff;
        assert!(!verify_password("correct horse", &stored));
    }

    #[test]
    fn tampered_iterations_rejected() {
        let mut stored = hash_password("correct horse");
        stored.iterations += 1;
        assert!(!verify_password("correct horse", &stored));
    }

    #[test]
    fn same_password_hashes_differ() {
        let first = hash_password("same password");
        let second = hash_password("same password");
        assert_ne!(first.hash, second.hash);
        assert_ne!(first.salt, second.salt);
        assert!(verify_password("same password", &first));
        assert!(verify_password("same password", &second));
    }

    #[test]
    fn passwd_file_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("passwd.json");
        let mut store = PasswdFile::load(&path).expect("load empty");
        assert!(store.is_empty());
        store.set_password("alice", "s3cret").expect("set password");
        let reloaded = PasswdFile::load(&path).expect("reload");
        assert!(reloaded.verify("alice", "s3cret"));
        assert!(!reloaded.verify("alice", "wrong"));
    }

    #[test]
    fn passwd_file_missing_loads_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = PasswdFile::load(&dir.path().join("does-not-exist.json"))
            .expect("missing file loads as empty");
        assert!(store.is_empty());
    }

    #[test]
    fn passwd_file_corrupt_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("passwd.json");
        std::fs::write(&path, "{ not valid json").expect("write corrupt file");
        assert!(PasswdFile::load(&path).is_err());
    }

    #[test]
    fn passwd_file_save_keeps_on_disk_shape() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("passwd.json");
        let mut store = PasswdFile::load(&path).expect("load empty");
        store.set_password("alice", "s3cret").expect("set password");
        let raw = std::fs::read_to_string(&path).expect("read passwd file");
        let doc: serde_json::Value = serde_json::from_str(&raw).expect("valid json");
        let users = doc.get("users").expect("top-level users object");
        let alice = users.get("alice").expect("alice entry");
        assert!(alice.get("salt").is_some(), "salt must be present");
        assert!(alice.get("hash").is_some(), "hash must be present");
        assert!(
            alice.get("iterations").is_some(),
            "iterations must be present"
        );
    }

    #[test]
    fn passwd_file_verify_unknown_user_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("passwd.json");
        let mut store = PasswdFile::load(&path).expect("load empty");
        store.set_password("alice", "s3cret").expect("set password");
        assert!(!store.verify("bob", "s3cret"));
        assert!(!store.verify("bob", "anything"));
    }
}
