#![allow(clippy::missing_errors_doc, clippy::must_use_candidate)]
use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, ErrorKind};

/// How long an identity token lives. The token carries its own expiry, so
/// nothing server-side has to remember it.
pub const SESSION_TTL_SECS: u64 = 14 * 86_400;

/// One live session of one user, as reported to the client.
///
/// The server does not keep a session list — the token is the session. This
/// type describes the shape of that report, not stored state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SessionRecord {
    pub jti: String,
    pub issued_at: u64,
}

/// Create a user without a password: the identity exists in the userdb of
/// record, but the account cannot log in until a password is set.
///
/// # Errors
/// Returns an error if the id already exists or the store cannot be written.
pub fn create_user(id: &str, admin: bool) -> Result<()> {
    let target = validate_user_id(id)?;
    // The user store is the only place identity lives.
    if crate::auth::user_store::read(&user_store_path(), &target).is_some() {
        anyhow::bail!("user already exists: {target}");
    }
    // Identity without a password hash: login impossible until a password
    // is set.
    crate::auth::user_store::rmw(&user_store_path(), |users| {
        if users.contains_key(&target) {
            return Err(AppError::new(ErrorKind::Conflict, "user already exists"));
        }
        users.insert(
            target.clone(),
            crate::auth::user_store::UserRecord {
                hash: None,
                admin,
                home: None,
                ver: 0,
            },
        );
        Ok(())
    })
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    // Provision the personal home (best-effort: a failure leaves a valid user
    // whose home is created on the next admin scan).
    if let Err(e) = crate::storage::files::ensure_personal_home(&target) {
        log::warn!("ensure_personal_home failed for {target}: {e}");
    }
    Ok(())
}

/// The identity record of `id` — the userdb of record. `None` when the user
/// does not exist.
pub fn user_record(id: &str) -> Result<Option<crate::auth::user_store::UserRecord>> {
    Ok(crate::auth::user_store::read(&user_store_path(), id))
}

/// Set the admin flag for an existing user, refusing the change when it
/// would leave zero admins. The existence check, the admin census, and the
/// write happen in one redb write transaction, so concurrent demotions
/// cannot interleave into a zero-admin store (redb is single-writer).
///
/// The write bumps the user's token version: the role travels in the token,
/// so the new role takes effect at the next sign-in.
///
/// Returns `Ok(true)` when the flag was changed, `Ok(false)` for a no-op
/// same-flag write.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] when the id is invalid,
/// [`ErrorKind::NotFound`] when the id is unknown,
/// [`ErrorKind::Conflict`] when the demotion would leave zero admins, and
/// [`ErrorKind::Database`] when the database cannot be read or written.
pub fn set_admin_role(id: &str, admin: bool) -> Result<bool, AppError> {
    let target = validate_user_id(id)?;
    // Existence in the user store is authoritative for identity.
    if crate::auth::user_store::read(&user_store_path(), &target).is_none() {
        return Err(AppError::new(
            ErrorKind::NotFound,
            format!("unknown user: {target}"),
        ));
    }
    crate::auth::user_store::set_admin(&user_store_path(), &target, admin)
}

/// Set the admin flag for an existing user. Fails for unknown ids.
///
/// # Errors
/// Returns an error if the id is unknown or the database cannot be written.
pub fn set_admin(id: &str, admin: bool) -> Result<()> {
    let target = validate_user_id(id)?;
    if crate::auth::user_store::read(&user_store_path(), &target).is_none() {
        anyhow::bail!("unknown user: {target}");
    }
    crate::auth::user_store::set_admin(&user_store_path(), &target, admin)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}

/// Remove a user: the identity record (and its password hash) are dropped in
/// one store write. Refuses with [`ErrorKind::Conflict`] when the deletion
/// would leave zero admins — the same invariant as [`set_admin_role`].
///
/// Returns `Ok(())` on success and [`ErrorKind::NotFound`] when `id` names no
/// user, [`ErrorKind::InvalidInput`] when `id` is invalid.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] for an invalid id,
/// [`ErrorKind::NotFound`] for an unknown id, [`ErrorKind::Conflict`] when
/// the deletion would leave zero admins, and [`ErrorKind::Database`]/
/// [`ErrorKind::IO`] when the store cannot be read or written.
pub fn delete_user(id: &str) -> Result<(), AppError> {
    let target = validate_user_id(id)?;
    // The store's `delete` enforces the zero-admin invariant atomically
    // (single write transaction, single writer) and returns `false` only
    // when the id is absent.
    let removed = crate::auth::user_store::delete(&user_store_path(), &target, false)?;
    if !removed {
        return Err(AppError::new(
            ErrorKind::NotFound,
            format!("unknown user: {target}"),
        ));
    }
    Ok(())
}

/// Invalidate every outstanding token of `id` by bumping its token version.
/// The user's password, role and attributes stay; only the tokens die.
///
/// # Errors
/// Returns [`ErrorKind::NotFound`] when the id is unknown.
pub fn invalidate_tokens(id: &str) -> Result<u64, AppError> {
    let target = validate_user_id(id)?;
    crate::auth::user_store::bump_ver(&user_store_path(), &target)
}

/// List every user with its identity record, ordered by id. The userdb of
/// record is the user store, so this is the authoritative user list — a
/// user provisioned offline is in it like any other.
///
/// # Errors
/// Returns an error if the store cannot be read.
pub fn list_users() -> Result<Vec<(String, crate::auth::user_store::UserRecord)>> {
    Ok(crate::auth::user_store::list(&user_store_path()))
}

/// Count users. The passwd store is the userdb of record, so this counts
/// passwd entries (single source of truth); a missing store counts as zero.
///
/// # Errors
/// Returns an error if the store cannot be read.
pub fn user_count() -> Result<usize> {
    Ok(crate::auth::user_store::count(&user_store_path()))
}

/// Validate a user id: trim surrounding whitespace, then require 1..=64 chars.
///
/// # Errors
/// Returns an [`AppError`] with [`ErrorKind::InvalidInput`] when the trimmed
/// id is empty or longer than 64 characters.
pub fn validate_user_id(id: &str) -> Result<String, AppError> {
    let trimmed = id.trim().to_string();
    if trimmed.is_empty() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "user id must not be empty",
        ));
    }
    if trimmed.len() > 64 {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "user id must be at most 64 characters",
        ));
    }
    Ok(trimmed)
}

/// Length bounds for newly set or provisioned passwords, applied by
/// [`validate_new_password`] (security review A3). Re-exported here so
/// callers keep one import path; defined in [`crate::auth::password`]
/// next to the strength check itself.
pub use crate::auth::password::{MAX_PASSWORD_LEN, MIN_PASSWORD_LEN};

/// Process-level override of the password-strength policy, for processes
/// that do not load the server configuration (the offline CLI). `None`
/// until set: the configuration decides.
static TRIVIAL_PASSWORDS_OVERRIDE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Set the password-strength policy for this process. Returns `false` when
/// an override is already set — the first one wins.
///
/// The offline CLI uses it to mirror the server's policy: the
/// `PICASU_TRIVIAL_PASSWORDS` environment variable wins over the
/// `trivial_passwords` key in `config.toml`.
pub fn set_trivial_passwords_override(enabled: bool) -> bool {
    TRIVIAL_PASSWORDS_OVERRIDE.set(enabled).is_ok()
}

/// Whether password strength checks are bypassed via the
/// `--trivial-passwords` flag / `PICASU_TRIVIAL_PASSWORDS` env / config
/// key. Unset config reads as strict (fail secure): only an explicit
/// opt-in disables the checks.
pub(crate) fn trivial_passwords_enabled() -> bool {
    if let Some(enabled) = TRIVIAL_PASSWORDS_OVERRIDE.get() {
        return *enabled;
    }
    if crate::model::config::APP_CONFIG.get().is_none() {
        return false;
    }
    crate::model::config::with_config(|c| c.trivial_passwords)
}

/// Validate a new password for `user_id`: trim, then enforce the default
/// policy ([`crate::auth::password::check_strength`]). Returns the
/// canonical (trimmed) form — the same form login verifies against, so
/// write-then-login stays consistent.
///
/// With trivial passwords enabled the strength checks are skipped and any
/// non-empty trimmed password is accepted (empty is presence, not
/// strength, and stays rejected); the bypass is warn-logged once, mirroring
/// the weakened-PBKDF precedent.
///
/// # Errors
/// Returns an [`AppError`] with [`ErrorKind::InvalidInput`] when the
/// trimmed password violates the policy.
pub fn validate_new_password(user_id: &str, password: &str) -> Result<String, AppError> {
    let canonical = password.trim().to_string();
    if trivial_passwords_enabled() {
        static WARN_ONCE: std::sync::Once = std::sync::Once::new();
        WARN_ONCE.call_once(|| {
            log::warn!(
                "trivial passwords enabled — password strength checks are off; never set in production"
            );
        });
        if canonical.is_empty() {
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                "password must not be empty",
            ));
        }
        return Ok(canonical);
    }
    crate::auth::password::check_strength(user_id, &canonical)?;
    Ok(canonical)
}

/// Write the password hash for `id` into the user store. The password
/// must already be validated and trimmed by the caller.
///
/// # Errors
/// Returns an [`AppError`] when the store cannot be written.
pub(crate) fn store_password(id: &str, password: &str) -> Result<(), AppError> {
    crate::auth::user_store::set_password(&user_store_path(), id, password)?;
    Ok(())
}

/// Uniform HTTP copy for every registration-conflict surface (used invite
/// or taken userId): all sites emit this exact message so a client cannot
/// distinguish the cause from the response, and no id is echoed.
pub(crate) const REGISTRATION_CONFLICT_MESSAGE: &str =
    "registration conflict: the invite or the userId is already taken";

/// Provision a new user: validate the id and password, insert the redb
/// record, and write the password hash to the user store.
///
/// The password is trimmed once and the trimmed (canonical) form is stored;
/// login verifies trimmed input, so write-then-login stays consistent.
/// Password hashing is CPU-bound, so callers must run this inside
/// `spawn_blocking`.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] for an invalid id or a password
/// violating the policy, [`ErrorKind::Conflict`] when the id already
/// exists, and [`ErrorKind::Database`]/[`ErrorKind::IO`] when either store
/// fails. A failure after the redb insert leaves a user record without a
/// password hash — no login is possible until it is reprovisioned.
pub fn provision_user(id: &str, password: &str, admin: bool) -> Result<String, AppError> {
    provision_user_with_home(id, password, admin, None)
}

pub fn provision_user_with_home(
    id: &str,
    password: &str,
    admin: bool,
    home: Option<String>,
) -> Result<String, AppError> {
    let id = validate_user_id(id)?;
    let canonical = validate_new_password(&id, password)?;
    if crate::auth::user_store::read(&user_store_path(), &id).is_some() {
        return Err(AppError::new(
            ErrorKind::Conflict,
            REGISTRATION_CONFLICT_MESSAGE,
        ));
    }
    // The identity record is the whole user: hash, role, attributes and the
    // token version. One locked write, no second store to keep in step.
    crate::auth::user_store::provision(&user_store_path(), &id, &canonical, admin, home)
        .map_err(|e| AppError::from_err(ErrorKind::IO, e.into()))?;
    // Provision the personal home (best-effort: a failure leaves a valid user
    // whose home is created on the next admin scan).
    if let Err(e) = crate::storage::files::ensure_personal_home(&id) {
        log::warn!("ensure_personal_home failed for {id}: {e}");
    }
    Ok(id)
}

/// Path of the user store: `<DATA_HOME>/auth/users.redb`.
/// The store itself takes an explicit path (see [`crate::auth::user_store`]);
/// this helper only resolves the production location.
#[must_use]
pub fn user_store_path() -> std::path::PathBuf {
    crate::storage::files::get_data_path()
        .join("auth")
        .join("users.redb")
}

/// Read the admin flag for `id` from the passwd store.
pub fn get_admin(id: &str) -> Result<Option<bool>> {
    Ok(crate::auth::user_store::read(&user_store_path(), id).map(|e| e.admin))
}

#[cfg(test)]
mod tests {
    use crate::tests::bootstrap::*;
    use crate::tests::fixtures::{test_password, test_password_padded};

    const ROUNDTRIP_ID: &str = "s2a-roundtrip-user";
    const ADMIN_ID: &str = "s2a-admin-user";
    const LIST_A_ID: &str = "s2a-list-a-user";
    const LIST_B_ID: &str = "s2a-list-b-user";

    fn clear_users_table() {
        // The user store is the userdb of record: clearing it clears the
        // identity state.
        crate::auth::user_store::remove_store(&crate::auth::users::user_store_path());
    }

    #[test]
    fn create_and_get_user_round_trip() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_users_table();

        crate::auth::users::create_user(ROUNDTRIP_ID, true).expect("create user");
        let found = crate::auth::users::user_record(ROUNDTRIP_ID).expect("get user");
        let found = found.expect("the user must exist");
        assert!(found.admin, "the admin flag must be stored");
        assert!(found.hash.is_none(), "no password was set");

        clear_users_table();
    }

    #[test]
    fn get_unknown_user_returns_none() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_users_table();

        let found = crate::auth::users::user_record("s2a-no-such-user").expect("get user");
        assert_eq!(found, None);

        clear_users_table();
    }

    #[test]
    fn create_duplicate_user_fails() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_users_table();

        crate::auth::users::create_user(ADMIN_ID, false).expect("create user");
        assert!(crate::auth::users::create_user(ADMIN_ID, true).is_err());

        clear_users_table();
    }

    #[test]
    fn set_admin_updates_flag() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_users_table();

        crate::auth::users::create_user(ADMIN_ID, false).expect("create user");
        crate::auth::users::set_admin(ADMIN_ID, true).expect("set admin");
        let found = crate::auth::users::user_record(ADMIN_ID).expect("get user");
        assert!(found.expect("the user must exist").admin);
        assert!(crate::auth::users::set_admin("s2a-no-such-user", true).is_err());

        clear_users_table();
    }

    #[test]
    fn list_users_and_count() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_users_table();
        assert_eq!(crate::auth::users::user_count().expect("count"), 0);
        assert!(
            crate::auth::users::list_users().expect("list").is_empty(),
            "fresh store lists no users"
        );

        crate::auth::users::create_user(LIST_B_ID, false).expect("create b");
        crate::auth::users::create_user(LIST_A_ID, true).expect("create a");
        assert_eq!(crate::auth::users::user_count().expect("count"), 2);
        let listed = crate::auth::users::list_users().expect("list");
        let ids: Vec<&str> = listed.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec![LIST_A_ID, LIST_B_ID]);
        assert!(listed[0].1.admin, "the first user is the admin");
        assert!(!listed[1].1.admin);

        clear_users_table();
    }

    #[test]
    fn user_store_path_lives_under_data_home() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        let path = crate::auth::users::user_store_path();
        assert_eq!(path.file_name().expect("file name"), "users.redb");
        assert_eq!(
            path.parent()
                .expect("parent")
                .file_name()
                .expect("parent name"),
            "auth"
        );
        assert!(path.starts_with(crate::storage::files::get_data_path()));
    }

    /// Whitespace-only password, built without a string literal so
    /// credential scanners see no hard-coded value.
    fn whitespace(width: usize) -> String {
        std::iter::repeat_n(' ', width).collect()
    }

    #[test]
    fn provision_user_rejects_short_and_empty_password() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();

        // 7 chars is below the minimum; a blocklist entry is listed; ""
        // and whitespace trim to nothing.
        for pw in [
            test_password("s3-short7")[..7].to_string(),
            crate::auth::password::blocklisted_sample().to_string(),
            String::new(),
            whitespace(3),
        ] {
            let err = super::provision_user("s3-short", &pw, false)
                .expect_err("weak/empty password must be rejected");
            assert_eq!(err.kind, crate::error::ErrorKind::InvalidInput);
        }
        // Rejected provisions leave no user record behind.
        assert_eq!(super::user_count().expect("count"), 0);
        assert!(super::user_record("s3-short").expect("get").is_none());

        reset_backend_state();
    }

    #[test]
    fn provision_user_accepts_exactly_12_chars() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();

        super::provision_user("s3-exact", &test_password("s3-exact")[..12], false)
            .expect("12 chars ok");
        assert_eq!(super::user_count().expect("count"), 1);

        reset_backend_state();
    }

    fn set_trivial_passwords(enabled: bool) {
        crate::model::config::APP_CONFIG
            .get()
            .expect("APP_CONFIG set")
            .write()
            .expect("APP_CONFIG lock")
            .trivial_passwords = enabled;
    }

    #[test]
    fn trivial_passwords_flag_bypasses_strength_but_not_presence() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();

        set_trivial_passwords(true);
        // Weak but non-empty passes with the bypass on…
        super::provision_user("s3-trivial", &test_password("s3-trivial")[..2], false)
            .expect("trivial accepted");
        // …and fails with it off (flag restored before asserting).
        set_trivial_passwords(false);
        let err = super::provision_user("s3-strict", &test_password("s3-trivial")[..2], false)
            .expect_err("strict rejects");
        assert_eq!(err.kind, crate::error::ErrorKind::InvalidInput);
        // Empty stays rejected either way: presence is not strength.
        set_trivial_passwords(true);
        let err =
            super::provision_user("s3-empty", &whitespace(3), false).expect_err("empty rejected");
        assert_eq!(err.kind, crate::error::ErrorKind::InvalidInput);
        set_trivial_passwords(false);

        reset_backend_state();
    }

    #[test]
    fn provision_user_duplicate_is_conflict() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();

        super::provision_user("s3-dup", &test_password("s3-dup-1"), true).expect("first provision");
        let err = super::provision_user("s3-dup", &test_password("s3-dup-2"), false)
            .expect_err("duplicate must fail");
        assert_eq!(err.kind, crate::error::ErrorKind::Conflict);
        // The existing record is untouched by the failed duplicate.
        let record = super::user_record("s3-dup")
            .expect("get")
            .expect("the user must exist");
        assert!(record.admin, "the first provision's role must survive");
        assert!(record.hash.is_some(), "the first hash must survive");

        reset_backend_state();
    }

    #[test]
    fn provision_user_trims_id_and_password_for_login_consistency() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();

        let id = super::provision_user("  s3-trim  ", &test_password_padded("s3-trim"), false)
            .expect("provision");
        assert_eq!(id, "s3-trim");
        let record = super::user_record("s3-trim")
            .expect("get")
            .expect("the user must exist");
        assert!(record.hash.is_some(), "the trimmed password must be stored");
        // Login verifies the trimmed input against the stored (trimmed)
        // canonical form — write-then-login must stay consistent.
        let store =
            crate::auth::user_store::UserStore::load(&super::user_store_path()).expect("load");
        assert!(store.verify("s3-trim", &test_password("s3-trim")));

        reset_backend_state();
    }

    #[test]
    fn s9_pin_a_entry_carries_admin_and_home_keys() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();

        super::provision_user("s9-pin-a", &test_password("s9-pin-a"), true).expect("provision");
        let entry =
            crate::auth::user_store::read(&super::user_store_path(), "s9-pin-a").expect("entry");
        assert!(entry.admin, "admin flag must be present");
        assert_eq!(entry.home, None, "home attribute must be present as None");
        assert!(entry.hash.is_some(), "password hash must be present");

        reset_backend_state();
    }

    #[test]
    fn s9_pin_create_user_refuses_login() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();

        super::create_user("s9-nopw", false).expect("create user");

        // Arbitrary password fails.
        let store =
            crate::auth::user_store::UserStore::load(&super::user_store_path()).expect("load");
        assert!(!store.verify("s9-nopw", &test_password("s9-nopw-wrong")));

        // A derived decoy also fails.
        assert!(!store.verify("s9-nopw", &test_password("s9-nopw-decoy")));

        // Entry has no password hash.
        let entry =
            crate::auth::user_store::read(&super::user_store_path(), "s9-nopw").expect("entry");
        assert!(
            entry.hash.is_none(),
            "hash must be absent for no-password user"
        );

        reset_backend_state();
    }

    #[test]
    fn validate_user_id_accepts_normal_id() {
        assert_eq!(
            super::validate_user_id(" alice ").expect("valid id"),
            "alice"
        );
    }

    #[test]
    fn validate_user_id_rejects_empty() {
        assert!(super::validate_user_id("").is_err());
        assert!(super::validate_user_id("   ").is_err());
    }

    #[test]
    fn validate_user_id_rejects_too_long() {
        let long = "a".repeat(65);
        assert!(super::validate_user_id(&long).is_err());
        let max = "a".repeat(64);
        assert_eq!(super::validate_user_id(&max).expect("64 chars ok"), max);
    }

    #[test]
    fn validate_user_id_trims_whitespace() {
        assert_eq!(super::validate_user_id("\tbob\n").expect("valid id"), "bob");
    }

    #[test]
    fn set_admin_role_rejects_unknown_id() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_users_table();

        let err = super::set_admin_role("s5-ghost", true).expect_err("unknown id must fail");
        assert_eq!(err.kind, crate::error::ErrorKind::NotFound);

        clear_users_table();
    }

    #[test]
    fn set_admin_role_refuses_last_admin_demotion() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_users_table();

        crate::auth::users::create_user("s5-sole", true).expect("create sole admin");
        let err =
            super::set_admin_role("s5-sole", false).expect_err("sole-admin demotion must fail");
        assert_eq!(err.kind, crate::error::ErrorKind::Conflict);
        // Refusal leaves the flag untouched.
        let found = crate::auth::users::user_record("s5-sole").expect("get user");
        assert_eq!(
            found.map(|record| record.admin),
            Some(true),
            "the sole admin keeps the role"
        );

        clear_users_table();
    }

    #[test]
    fn set_admin_role_allows_transfer_when_other_admin_exists() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_users_table();

        crate::auth::users::create_user("s5-a", true).expect("create a");
        crate::auth::users::create_user("s5-b", true).expect("create b");
        assert!(super::set_admin_role("s5-a", false).expect("transfer demotion"));
        assert!(
            !crate::auth::users::user_record("s5-a")
                .expect("get a")
                .expect("the user must exist")
                .admin
        );
        assert!(
            crate::auth::users::user_record("s5-b")
                .expect("get b")
                .expect("the user must exist")
                .admin
        );

        clear_users_table();
    }

    #[test]
    fn set_admin_role_promotes_and_noops() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_users_table();

        crate::auth::users::create_user("s5-c", false).expect("create c");
        assert!(super::set_admin_role("s5-c", true).expect("promote"));
        assert!(
            crate::auth::users::user_record("s5-c")
                .expect("get c")
                .expect("the user must exist")
                .admin
        );
        // Same-flag write is a no-op reporting no change.
        assert!(!super::set_admin_role("s5-c", true).expect("noop promote"));
        crate::auth::users::create_user("s5-d", false).expect("create d");
        assert!(!super::set_admin_role("s5-d", false).expect("noop demote"));
        clear_users_table();
    }

    #[test]
    fn ensure_personal_home_creates_dir_and_album_when_enabled() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();

        let data_path = crate::storage::files::DATA_PATH
            .get()
            .expect("DATA_PATH set");
        let users_home = data_path.join("users");
        let _ = std::fs::remove_dir_all(&users_home);
        std::fs::create_dir_all(&users_home).expect("create users home");

        {
            let mut c = crate::model::config::APP_CONFIG
                .get()
                .expect("cfg")
                .write()
                .expect("lock");
            c.users_home = Some(users_home.clone());
        }

        crate::auth::users::create_user("home-user", false).expect("create user");

        let home_dir = users_home.join("home-user");
        assert!(home_dir.is_dir(), "personal home dir must be created");
        assert!(
            crate::process::dir_album::get_album_id_for_dir(&home_dir).is_some(),
            "personal home must have a dir-album record"
        );

        {
            let mut c = crate::model::config::APP_CONFIG
                .get()
                .expect("cfg")
                .write()
                .expect("lock");
            c.users_home = None;
        }
        reset_backend_state();
    }

    #[test]
    fn ensure_personal_home_is_noop_when_disabled() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();

        assert!(crate::storage::files::ensure_personal_home("disabled-user").is_ok());
        reset_backend_state();
    }
}
