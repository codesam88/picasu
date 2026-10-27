use std::collections::HashSet;
use std::sync::LazyLock;
#[cfg(test)]
use std::sync::atomic::AtomicU64;

use pbkdf2::pbkdf2_hmac;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use argon2::{
    Argon2, PasswordHasher, PasswordVerifier, password_hash::phc::PasswordHash as ArgonPasswordHash,
};

use crate::error::{AppError, ErrorKind};

/// Length of the per-password random salt in bytes (legacy PBKDF2 rows).
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

/// Environment variable selecting trivial argon2id cost for test runs.
/// Only the literal value `1` enables it; unset, empty, or any other
/// value keeps production params. Read per hash call so tests can flip
/// it within one process. Env-only so it cannot arrive via a config
/// file. Production must never set this — hashes become cheap to crack.
pub const WEAK_PWHASH_ENV_VAR: &str = "PICASU_WEAK_PWHASH";

static DUMMY_HASH: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| hash_password("dummy"));

/// PHC string used by the unknown-user dummy verify. Generated lazily at
/// production or weak cost depending on the environment.
pub(crate) fn dummy_hash() -> &'static str {
    DUMMY_HASH.as_str()
}

/// True when the weak-cost override is active (env reads `1`).
#[must_use]
pub fn weak_pwhash_enabled() -> bool {
    std::env::var(WEAK_PWHASH_ENV_VAR).ok().as_deref() == Some("1")
}

/// Build the argon2id hasher at production or trivial cost depending on
/// the weak override (read per call).
fn argon2_hasher() -> Argon2<'static> {
    if weak_pwhash_enabled() {
        static WARN_ONCE: std::sync::Once = std::sync::Once::new();
        WARN_ONCE.call_once(|| {
            log::warn!(
                "{WEAK_PWHASH_ENV_VAR}=1: weakened password hashing for tests — never set in production"
            );
        });
        Argon2::new(
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            argon2::Params::new(8192, 1, 1, None).expect("valid params"),
        )
    } else {
        Argon2::default()
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

/// Hash a password with argon2id at production cost (or trivial cost when
/// the weak override is active) and a per-hash random salt. Returns the
/// PHC-encoded string stored in the user record.
///
/// # Panics
/// Panics only if the system RNG fails, which indicates a broken host.
#[must_use]
pub fn hash_password(password: &str) -> String {
    argon2_hasher()
        .hash_password(password.as_bytes())
        .expect("argon2 hashing cannot fail for valid inputs")
        .to_string()
}

/// Verify a candidate password against a stored value, constant-time in
/// both paths.
///
/// Dual-path by design (legacy rows are not migrated): a stored value
/// beginning with the argon2 PHC prefix `$argon2id$` verifies through
/// argon2; anything else is treated as a legacy PBKDF2 hash serialized
/// as JSON and verified through PBKDF2-HMAC-SHA256.
#[must_use]
pub fn verify_password(password: &str, stored: &str) -> bool {
    if stored.starts_with("$argon2id$") {
        let Ok(parsed) = ArgonPasswordHash::new(stored) else {
            return false;
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    } else if let Ok(legacy) = serde_json::from_str::<PasswordHash>(stored) {
        let candidate = derive(password, &legacy.salt, legacy.iterations);
        candidate.ct_eq(&legacy.hash).into()
    } else {
        false
    }
}

/// Hash a share password with argon2id at the OWASP-recommended minimum
/// cost (m=19456 KiB, t=2, p=1) and a per-hash random salt. Returns the
/// PHC-encoded string stored in `Share.password`.
///
/// # Panics
/// Panics only if the system RNG fails, which indicates a broken host.
#[must_use]
pub fn hash_share_password(password: &str) -> String {
    argon2_hasher()
        .hash_password(password.as_bytes())
        .expect("argon2 hashing cannot fail for valid inputs")
        .to_string()
}

/// Verify a candidate share password against a stored value.
///
/// Dual-path by design (no migration): a stored value beginning with the
/// argon2 PHC prefix `$argon2id$` is verified through argon2; anything
/// else is treated as a legacy plaintext value and compared verbatim.
#[must_use]
pub fn verify_share_password(candidate: &str, stored: &str) -> bool {
    if stored.starts_with("$argon2id$") {
        let Ok(parsed) = ArgonPasswordHash::new(stored) else {
            return false;
        };
        Argon2::default()
            .verify_password(candidate.as_bytes(), &parsed)
            .is_ok()
    } else {
        candidate == stored
    }
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

/// The identity record lives in [`crate::auth::user_store`] — the store
/// owns its row type. This module keeps the hash primitives.
///
/// Best-effort `0600` on the user store file; permission errors are
/// ignored and non-unix platforms are a no-op.
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
    fn hash_verify_roundtrip() {
        let stored = hash_password(&test_password("t-ok"));
        assert!(stored.starts_with("$argon2id$"));
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
    fn tampered_hash_rejected() {
        let mut stored = hash_password(&test_password("t-ok"));
        // Flip a character in the PHC string.
        let mut chars: Vec<char> = stored.chars().collect();
        if let Some(c) = chars.last_mut() {
            *c = if *c == 'a' { 'b' } else { 'a' };
        }
        stored = chars.into_iter().collect();
        assert!(!verify_password(&test_password("t-ok"), &stored));
    }

    #[test]
    fn same_password_hashes_differ() {
        let first = hash_password(&test_password("t-same"));
        let second = hash_password(&test_password("t-same"));
        assert_ne!(first, second);
        assert!(verify_password(&test_password("t-same"), &first));
        assert!(verify_password(&test_password("t-same"), &second));
    }
}
