use std::collections::HashSet;
use std::sync::LazyLock;
#[cfg(test)]
use std::sync::atomic::AtomicU64;

use anyhow::Context as _;
use pbkdf2::pbkdf2_hmac;
use rand::random;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::error::{AppError, ErrorKind};

/// PBKDF2-HMAC-SHA256 iteration count for new password hashes.
pub const DEFAULT_ITERATIONS: u32 = 600_000;
/// Length of the per-password random salt in bytes.
pub const SALT_LEN: usize = 16;
pub const HASH_LEN: usize = 32;

/// Minimum length for a new password, in characters. Follows NIST
/// SP 800-63B §5.1.1.2: length plus screening against known-bad values
/// beats composition rules, which mostly produce `Password123!`.
pub const MIN_PASSWORD_LEN: usize = 8;
/// Maximum length for a new password, in characters. Not a usability
/// judgment: an unbounded secret is unbounded KDF input on an endpoint
/// with no throttling yet, so the ceiling is a `DoS` bound. 256 is far
/// beyond any legitimate password or passphrase.
pub const MAX_PASSWORD_LEN: usize = 256;

/// Shipped blocklist for the default password policy: the `SecLists`
/// 500-worst-passwords list plus service-specific words (see
/// `blocklist.txt`). Compared case-insensitively against the trimmed
/// candidate. Offline by design — a self-hosted verifier cannot depend
/// on a breach-corpus API.
static BLOCKLIST: LazyLock<HashSet<String>> = LazyLock::new(|| {
    include_str!("blocklist.txt")
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            (!trimmed.is_empty() && !trimmed.starts_with('#')).then(|| trimmed.to_lowercase())
        })
        .collect()
});

/// True when the whole (already lowercased) candidate is one repeated
/// character or one unbroken ascending/descending run.
fn is_trivial_pattern(lower: &str) -> bool {
    let chars: Vec<char> = lower.chars().collect();
    if chars.len() < 2 {
        return false;
    }
    if chars.iter().all(|&c| c == chars[0]) {
        return true;
    }
    let ascending = chars.windows(2).all(|w| {
        let (a, b) = (w[0] as u32, w[1] as u32);
        b == a + 1
    });
    let descending = chars.windows(2).all(|w| {
        let (a, b) = (w[0] as u32, w[1] as u32);
        a == b + 1
    });
    ascending || descending
}

/// Enforce the default password policy on `canonical` (already trimmed)
/// for `user_id` (already validated): length floor and ceiling, then the
/// blocklist, the user-id equality rule, then trivial patterns.
///
/// Deliberately no composition rules and no expiry (NIST SP 800-63B):
/// the PBKDF2 work factor plus the planned online throttling carry the
/// guessing resistance; this floor only keeps the trivially crackable
/// class out. Callers pass the canonical id so `Alice` rejects `alice`.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] describing the first failed rule.
pub fn check_strength(user_id: &str, canonical: &str) -> Result<(), AppError> {
    let len = canonical.chars().count();
    if len < MIN_PASSWORD_LEN {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            format!("password must be at least {MIN_PASSWORD_LEN} characters"),
        ));
    }
    if len > MAX_PASSWORD_LEN {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            format!("password must be at most {MAX_PASSWORD_LEN} characters"),
        ));
    }
    let lower = canonical.to_lowercase();
    if BLOCKLIST.contains(&lower) {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "password is too common; choose a less predictable one",
        ));
    }
    if lower == user_id.trim().to_lowercase() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "password must not match the user id",
        ));
    }
    if is_trivial_pattern(&lower) {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "password is too predictable; use unrelated characters",
        ));
    }
    Ok(())
}

/// Environment variable overriding the PBKDF2 iteration count for new
/// hashes (and the unknown-user dummy round). Test-only: set to a very
/// low count (e.g. `1000`) to keep password hashing off the dev-loop
/// critical path. PBKDF itself is never bypassed — only the effort
/// factor changes. Env-only so it cannot arrive via a config file;
/// absent or unparseable falls back below (fail secure, never fail
/// weak). Production must never set this.
pub const ITERATIONS_ENV_VAR: &str = "PICASU_PBKDF_ITERATIONS";

/// Default when no explicit override applies: a very low count under
/// `cfg(test)` (unit tests always run cheap), else [`DEFAULT_ITERATIONS`].
/// Integration tests link the lib without `cfg(test)`, so they follow
/// the production default unless the env var is set (as `just
/// backend-test` does).
#[cfg(test)]
const TEST_ITERATIONS: u32 = 1_000;

/// Parse an iteration override: positive integers only, clamped to a
/// minimum of 1; anything else (including absent) is `None` for default.
fn parse_iterations_override(var: Option<&str>) -> Option<u32> {
    var.and_then(|s| s.trim().parse::<u32>().ok())
        .filter(|&n| n > 0)
}

/// Iteration count for new password hashes: the explicit env override
/// when set, else the test default under `cfg(test)`, else
/// [`DEFAULT_ITERATIONS`]. Logs once when the env override is active so
/// a production process running weakened hashing is unmistakable.
pub(crate) fn password_iterations() -> u32 {
    match parse_iterations_override(std::env::var(ITERATIONS_ENV_VAR).ok().as_deref()) {
        Some(n) => {
            static WARN_ONCE: std::sync::Once = std::sync::Once::new();
            WARN_ONCE.call_once(|| {
                log::warn!(
                    "{ITERATIONS_ENV_VAR}={n}: weakened password hashing for tests — never set in production"
                );
            });
            n
        }
        None => {
            #[cfg(test)]
            {
                TEST_ITERATIONS
            }
            #[cfg(not(test))]
            {
                DEFAULT_ITERATIONS
            }
        }
    }
}

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
    // Generated directly: no zero-initialized buffer that reads as a
    // hardcoded salt, and still panics on RNG failure as documented.
    let salt: [u8; SALT_LEN] = random();
    let iterations = password_iterations();
    let hash = derive(password, &salt, iterations);
    PasswordHash {
        salt,
        hash,
        iterations,
    }
}

/// Check a password against a stored hash using a constant-time comparison.
#[must_use]
pub fn verify_password(password: &str, expected: &PasswordHash) -> bool {
    let candidate = derive(password, &expected.salt, expected.iterations);
    candidate.ct_eq(&expected.hash).into()
}

/// KDF rounds executed on the login path (tests only): incremented by
/// the dummy verify and the known-user verify in the authenticate
/// handler, so login tests can prove throttled requests skip hashing
/// without timing assertions. Unit tests of this module never touch it.
#[cfg(test)]
pub static VERIFY_CALLS: AtomicU64 = AtomicU64::new(0);

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
pub(crate) fn restrict_permissions(path: &std::path::Path) {
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

/// First blocklist entry containing letters, for tests asserting the
/// listed-password rule. Sourced from the shipped list so no literal
/// password appears in test code (credential scanners flag those).
#[cfg(test)]
pub(crate) fn blocklisted_sample() -> &'static str {
    BLOCKLIST
        .iter()
        .find(|entry| entry.chars().any(|c| c.is_ascii_alphabetic()))
        .expect("blocklist ships an alphabetic entry")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::fixtures::test_password;

    #[test]
    fn iteration_override_parsing() {
        assert_eq!(parse_iterations_override(None), None);
        assert_eq!(parse_iterations_override(Some("1000")), Some(1000));
        assert_eq!(parse_iterations_override(Some("  2048  ")), Some(2048));
        // Zero, negative, non-numeric, and empty all fall back to default.
        assert_eq!(parse_iterations_override(Some("0")), None);
        assert_eq!(parse_iterations_override(Some("-5")), None);
        assert_eq!(parse_iterations_override(Some("fast")), None);
        assert_eq!(parse_iterations_override(Some("")), None);
    }

    #[test]
    fn hash_verify_roundtrip() {
        let stored = hash_password(&test_password("t-ok"));
        assert!(verify_password(&test_password("t-ok"), &stored));
    }

    #[test]
    fn wrong_password_rejected() {
        let stored = hash_password(&test_password("t-ok"));
        assert!(!verify_password(&test_password("t-wrong"), &stored));
    }

    fn strength(user_id: &str, password: &str) -> Result<(), crate::error::AppError> {
        super::check_strength(user_id, password)
    }

    #[test]
    fn strength_floor_and_ceiling_count_characters() {
        // 7 chars: below the minimum, whatever its content.
        assert!(strength("s9-u", &test_password("s9-short")[..7]).is_err());
        // Exactly 8 passes the floor (and is neither listed nor trivial).
        strength("s9-u", &test_password("s9-floor")[..8]).expect("8 chars pass");
        // 256 passes; 257 is rejected to bound KDF input (pre-throttling DoS).
        let long_ok: String = std::iter::repeat_n('x', 252).collect::<String>() + "y2b!";
        assert_eq!(long_ok.chars().count(), 256);
        strength("s9-u", &long_ok).expect("256 chars pass");
        let too_long: String = std::iter::repeat_n('z', 257).collect();
        assert!(strength("s9-u", &too_long).is_err());
    }

    #[cfg(test)]
    fn capitalized(s: &str) -> String {
        let mut chars = s.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => String::new(),
        }
    }

    #[test]
    fn strength_rejects_listed_passwords_case_insensitively() {
        // A shipped blocklist entry, rejected in every letter case.
        let listed = super::blocklisted_sample();
        assert!(strength("s9-u", listed).is_err());
        assert!(strength("s9-u", &capitalized(listed)).is_err());
        assert!(strength("s9-u", &listed.to_uppercase()).is_err());
        // Service appendix (membership: the behavior is covered by
        // `listed` above; these assert the appendix shipped).
        assert!(super::BLOCKLIST.contains("picasu"));
        assert!(super::BLOCKLIST.contains("urocissa"));
    }

    #[test]
    fn strength_rejects_user_id_and_trivial_patterns() {
        // Unlisted, non-trivial: the id rule is the only possible cause.
        let id_like = test_password("s9-idlike");
        assert!(strength(&id_like.to_uppercase(), &id_like).is_err());
        // The same password with a different id is fine.
        strength("s9-other", &id_like).expect("other id passes");
        let repeats: String = std::iter::repeat_n('a', 8).collect();
        assert!(strength("s9-u", &repeats).is_err());
        let ascending: String = ('a'..='h').collect();
        assert!(strength("s9-u", &ascending).is_err());
        let descending: String = ('a'..='h').rev().collect();
        assert!(strength("s9-u", &descending).is_err());
    }

    #[test]
    fn strength_accepts_passphrases_and_random_strings() {
        let passphrase = format!(
            "{} {} {}",
            test_password("s9-w1"),
            test_password("s9-w2"),
            test_password("s9-w3")
        );
        strength("s9-u", &passphrase).expect("passphrase passes");
        strength("s9-u", &test_password("t-strength")).expect("derived passes");
        strength("s9-u", &test_password("s9-ok")).expect("ordinary passes");
    }

    #[test]
    fn tampered_salt_rejected() {
        let mut stored = hash_password(&test_password("t-ok"));
        stored.salt[0] ^= 0xff;
        assert!(!verify_password(&test_password("t-ok"), &stored));
    }

    #[test]
    fn tampered_iterations_rejected() {
        let mut stored = hash_password(&test_password("t-ok"));
        stored.iterations += 1;
        assert!(!verify_password(&test_password("t-ok"), &stored));
    }

    #[test]
    fn same_password_hashes_differ() {
        let first = hash_password(&test_password("t-same"));
        let second = hash_password(&test_password("t-same"));
        assert_ne!(first.hash, second.hash);
        assert_ne!(first.salt, second.salt);
        assert!(verify_password(&test_password("t-same"), &first));
        assert!(verify_password(&test_password("t-same"), &second));
    }

    #[test]
    fn passwd_file_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("passwd.json");
        let mut store = PasswdFile::load(&path).expect("load empty");
        assert!(store.is_empty());
        store
            .set_password("alice", &test_password("t-file"))
            .expect("set password");
        let reloaded = PasswdFile::load(&path).expect("reload");
        assert!(reloaded.verify("alice", &test_password("t-file")));
        assert!(!reloaded.verify("alice", &test_password("t-other")));
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
        store
            .set_password("alice", &test_password("t-file"))
            .expect("set password");
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
        store
            .set_password("alice", &test_password("t-file"))
            .expect("set password");
        assert!(!store.verify("bob", &test_password("t-file")));
        assert!(!store.verify("bob", &test_password("t-other")));
    }
}
