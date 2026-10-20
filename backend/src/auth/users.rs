use anyhow::{Context, Result};
use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, ErrorKind};
use crate::storage::db::{TREE, USERS};

/// How long a server-side session record lives. Matches the identity-token
/// TTL in [`crate::router::auth::Claims::new_user`] (14 days).
pub const SESSION_TTL_SECS: u64 = 14 * 86_400;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SessionRecord {
    pub jti: String,
    pub issued_at: u64,
}

/// A user record stored in the `USERS` redb table, keyed by user id.
/// Deliberately minimal; extended with new optional fields later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserRecord {
    pub admin: bool,
    #[serde(default)]
    pub sessions: Vec<SessionRecord>,
}

/// Create a user. Fails if the id already exists.
///
/// Opening the table on a write transaction auto-creates it on first use.
///
/// # Errors
/// Returns an error if the id already exists or the database cannot be written.
pub fn create_user(id: &str, admin: bool) -> Result<()> {
    let txn = TREE
        .in_disk
        .begin_write()
        .context("Failed to begin write transaction for USERS")?;
    {
        let mut table = txn
            .open_table(USERS)
            .context("Failed to open USERS for write")?;
        if table.get(id)?.is_some() {
            anyhow::bail!("user already exists: {id}");
        }
        let value = serde_json::to_string(&UserRecord {
            admin,
            sessions: Vec::new(),
        })
        .context("Failed to serialize UserRecord")?;
        table
            .insert(id, value.as_str())
            .context("Failed to insert into USERS")?;
    }
    txn.commit().context("Failed to commit USERS insert")
}

/// Look up a user by id. A never-created table reads as empty
/// (tables are only created on write paths, never here).
///
/// # Errors
/// Returns an error if the database cannot be read or a row is corrupt.
pub fn get_user(id: &str) -> Result<Option<UserRecord>> {
    let txn = TREE
        .in_disk
        .begin_read()
        .context("Failed to begin read transaction for USERS")?;
    let table = match txn.open_table(USERS) {
        Ok(table) => table,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(e) => return Err(e).context("Failed to open USERS")?,
    };
    match table.get(id)? {
        Some(guard) => {
            let record: UserRecord =
                serde_json::from_str(guard.value()).context("Failed to deserialize UserRecord")?;
            Ok(Some(record))
        }
        None => Ok(None),
    }
}

/// Set the admin flag for an existing user, refusing the change when it
/// would leave zero admins. The existence check, the admin census, and the
/// write happen in one redb write transaction, so concurrent demotions
/// cannot interleave into a zero-admin store (redb is single-writer).
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
    use redb::ReadableTable;
    let target = validate_user_id(id)?;
    let txn = TREE
        .in_disk
        .begin_write()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    let changed = {
        let mut table = txn
            .open_table(USERS)
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
        let current: UserRecord = match table
            .get(target.as_str())
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?
        {
            Some(guard) => serde_json::from_str(guard.value()).map_err(|e| {
                AppError::from_err(ErrorKind::Database, Into::<anyhow::Error>::into(e))
            })?,
            None => {
                return Err(AppError::new(
                    ErrorKind::NotFound,
                    format!("unknown user: {target}"),
                ));
            }
        };
        if current.admin == admin {
            return Ok(false);
        }
        if !admin {
            let mut admins = 0;
            for entry in table
                .iter()
                .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?
            {
                let (_, guard) =
                    entry.map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
                let record: UserRecord = serde_json::from_str(guard.value()).map_err(|e| {
                    AppError::from_err(ErrorKind::Database, Into::<anyhow::Error>::into(e))
                })?;
                if record.admin {
                    admins += 1;
                }
            }
            if admins <= 1 {
                return Err(AppError::new(
                    ErrorKind::Conflict,
                    "demotion would leave zero admins",
                ));
            }
        }
        let value = serde_json::to_string(&UserRecord {
            admin,
            sessions: current.sessions.clone(),
        })
        .map_err(|e| AppError::from_err(ErrorKind::Database, Into::<anyhow::Error>::into(e)))?;
        table
            .insert(target.as_str(), value.as_str())
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
        true
    };
    txn.commit()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    Ok(changed)
}

/// Set the admin flag for an existing user. Fails for unknown ids.
///
/// # Errors
/// Returns an error if the id is unknown or the database cannot be written.
pub fn set_admin(id: &str, admin: bool) -> Result<()> {
    let txn = TREE
        .in_disk
        .begin_write()
        .context("Failed to begin write transaction for USERS")?;
    {
        let mut table = txn
            .open_table(USERS)
            .context("Failed to open USERS for write")?;
        let current: UserRecord = match table.get(id)? {
            Some(guard) => {
                serde_json::from_str(guard.value()).context("Failed to deserialize UserRecord")?
            }
            None => anyhow::bail!("unknown user: {id}"),
        };
        let value = serde_json::to_string(&UserRecord {
            admin,
            sessions: current.sessions,
        })
        .context("Failed to serialize UserRecord")?;
        table
            .insert(id, value.as_str())
            .context("Failed to update USERS")?;
    }
    txn.commit().context("Failed to commit USERS update")
}

#[allow(clippy::cast_sign_loss)]
fn now_secs() -> u64 {
    (chrono::Utc::now().timestamp_millis() / 1000) as u64
}

/// Add a live session for `id` with `jti`. Deduplicates by `jti` (refreshing
/// `issued_at` and moving to the back), pushes the new/ refreshed entry, and
/// lazily prunes expired sessions (`issued_at + SESSION_TTL_SECS <= now`) in
/// the same write transaction.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] when the id is invalid,
/// [`ErrorKind::NotFound`] when the id is unknown, and
/// [`ErrorKind::Database`] when the database cannot be read or written.
pub fn add_session(id: &str, jti: &str) -> Result<(), AppError> {
    let target = validate_user_id(id)?;
    if jti.is_empty() {
        return Err(AppError::new(
            ErrorKind::InvalidInput,
            "jti must not be empty",
        ));
    }
    let now = now_secs();
    let txn = TREE
        .in_disk
        .begin_write()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    {
        let mut table = txn
            .open_table(USERS)
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
        let mut record: UserRecord = match table
            .get(target.as_str())
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?
        {
            Some(guard) => serde_json::from_str(guard.value()).map_err(|e| {
                AppError::from_err(ErrorKind::Database, Into::<anyhow::Error>::into(e))
            })?,
            None => {
                return Err(AppError::new(
                    ErrorKind::NotFound,
                    format!("unknown user: {target}"),
                ));
            }
        };
        // Lazy-prune expired sessions.
        record
            .sessions
            .retain(|s| s.issued_at + SESSION_TTL_SECS > now);
        // Dedupe by jti: remove existing, refresh issued_at and move to back.
        if let Some(pos) = record.sessions.iter().position(|s| s.jti == jti) {
            record.sessions.remove(pos);
        }
        record.sessions.push(SessionRecord {
            jti: jti.to_string(),
            issued_at: now,
        });
        let value = serde_json::to_string(&record)
            .map_err(|e| AppError::from_err(ErrorKind::Database, Into::<anyhow::Error>::into(e)))?;
        table
            .insert(target.as_str(), value.as_str())
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    }
    txn.commit()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    Ok(())
}

/// Remove the session `jti` for `id`.
///
/// Returns `Ok(true)` when a session was removed, `Ok(false)` when the user
/// or the session does not exist.
///
/// # Errors
/// Returns [`ErrorKind::Database`] when the database cannot be read or written.
pub fn remove_session(id: &str, jti: &str) -> Result<bool, AppError> {
    // Validate id shape; invalid ids are treated as unknown → Ok(false).
    let target = match validate_user_id(id) {
        Ok(v) => v,
        Err(e) if e.kind == ErrorKind::InvalidInput => return Ok(false),
        Err(e) => return Err(e),
    };
    if jti.is_empty() {
        return Ok(false);
    }
    let txn = TREE
        .in_disk
        .begin_write()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    let removed = {
        let mut table = txn
            .open_table(USERS)
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
        let mut record: UserRecord = match table
            .get(target.as_str())
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?
        {
            Some(guard) => serde_json::from_str(guard.value()).map_err(|e| {
                AppError::from_err(ErrorKind::Database, Into::<anyhow::Error>::into(e))
            })?,
            None => return Ok(false),
        };
        let before = record.sessions.len();
        record.sessions.retain(|s| s.jti != jti);
        let removed = record.sessions.len() != before;
        if removed {
            let value = serde_json::to_string(&record).map_err(|e| {
                AppError::from_err(ErrorKind::Database, Into::<anyhow::Error>::into(e))
            })?;
            table
                .insert(target.as_str(), value.as_str())
                .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
        }
        removed
    };
    txn.commit()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    Ok(removed)
}

/// Clear all live sessions for `id`.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] when the id is invalid,
/// [`ErrorKind::NotFound`] when the id is unknown, and
/// [`ErrorKind::Database`] when the database cannot be read or written.
pub fn clear_sessions(id: &str) -> Result<(), AppError> {
    let target = validate_user_id(id)?;
    let txn = TREE
        .in_disk
        .begin_write()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    {
        let mut table = txn
            .open_table(USERS)
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
        let mut record: UserRecord = match table
            .get(target.as_str())
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?
        {
            Some(guard) => serde_json::from_str(guard.value()).map_err(|e| {
                AppError::from_err(ErrorKind::Database, Into::<anyhow::Error>::into(e))
            })?,
            None => {
                return Err(AppError::new(
                    ErrorKind::NotFound,
                    format!("unknown user: {target}"),
                ));
            }
        };
        record.sessions.clear();
        let value = serde_json::to_string(&record)
            .map_err(|e| AppError::from_err(ErrorKind::Database, Into::<anyhow::Error>::into(e)))?;
        table
            .insert(target.as_str(), value.as_str())
            .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    }
    txn.commit()
        .map_err(|e| AppError::from_err(ErrorKind::Database, e.into()))?;
    Ok(())
}

/// List all users as `(id, record)` pairs ordered by id.
/// A never-created table reads as empty.
///
/// # Errors
/// Returns an error if the database cannot be read or a row is corrupt.
pub fn list_users() -> Result<Vec<(String, UserRecord)>> {
    let txn = TREE
        .in_disk
        .begin_read()
        .context("Failed to begin read transaction for USERS")?;
    let table = match txn.open_table(USERS) {
        Ok(table) => table,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(e) => return Err(e).context("Failed to open USERS")?,
    };
    let mut users = Vec::new();
    for entry in table.iter()? {
        let (key, guard) = entry?;
        let record: UserRecord =
            serde_json::from_str(guard.value()).context("Failed to deserialize UserRecord")?;
        users.push((key.value().to_string(), record));
    }
    users.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(users)
}

/// Count users. A never-created table counts as zero.
///
/// # Errors
/// Returns an error if the database cannot be read.
pub fn user_count() -> Result<usize> {
    let txn = TREE
        .in_disk
        .begin_read()
        .context("Failed to begin read transaction for USERS")?;
    let table = match txn.open_table(USERS) {
        Ok(table) => table,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(0),
        Err(e) => return Err(e).context("Failed to open USERS")?,
    };
    usize::try_from(table.len()?).context("USERS row count exceeds usize")
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

/// Whether password strength checks are bypassed via the
/// `--trivial-passwords` flag / `PICASU_TRIVIAL_PASSWORDS` env / config
/// key. Unset config reads as strict (fail secure): only an explicit
/// opt-in disables the checks.
pub(crate) fn trivial_passwords_enabled() -> bool {
    crate::model::config::APP_CONFIG
        .get()
        .and_then(|config| config.read().ok().map(|guard| guard.trivial_passwords))
        .unwrap_or(false)
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

/// Write the password hash for `id` into `passwd.json`, creating the auth
/// directory on first use. The password must already be validated and
/// trimmed by the caller.
///
/// # Errors
/// Returns an [`AppError`] with [`ErrorKind::IO`] when the directory or
/// the store cannot be written.
pub(crate) fn store_password(id: &str, password: &str) -> Result<(), AppError> {
    let path = passwd_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::from_err(ErrorKind::IO, e.into()))?;
    }
    let mut store = crate::auth::password::PasswdFile::load(&path)
        .map_err(|e| AppError::from_err(ErrorKind::IO, e))?;
    store
        .set_password(id, password)
        .map_err(|e| AppError::from_err(ErrorKind::IO, e))
}

/// Uniform HTTP copy for every registration-conflict surface (used invite
/// or taken userId): all sites emit this exact message so a client cannot
/// distinguish the cause from the response, and no id is echoed.
pub(crate) const REGISTRATION_CONFLICT_MESSAGE: &str =
    "registration conflict: the invite or the userId is already taken";

/// Provision a new user: validate the id and password, insert the redb
/// record, and write the password hash to `passwd.json`.
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
    let id = validate_user_id(id)?;
    let canonical = validate_new_password(&id, password)?;
    if get_user(&id)
        .map_err(|e| AppError::from_err(ErrorKind::Database, e))?
        .is_some()
    {
        return Err(AppError::new(
            ErrorKind::Conflict,
            REGISTRATION_CONFLICT_MESSAGE,
        ));
    }
    create_user(&id, admin).map_err(|e| AppError::from_err(ErrorKind::Database, e))?;
    store_password(&id, &canonical)?;
    Ok(id)
}

/// Path of the JSON password store: `<DATA_HOME>/auth/passwd.json`.
/// The store itself takes an explicit path (see [`crate::auth::password`]);
/// this helper only resolves the production location.
#[must_use]
pub fn passwd_file_path() -> std::path::PathBuf {
    crate::storage::files::get_data_path()
        .join("auth")
        .join("passwd.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::TREE;
    use crate::storage::db::USERS;
    use crate::tests::bootstrap::*;
    use redb::{ReadableTable, TableDefinition};

    const ROUNDTRIP_ID: &str = "s2a-roundtrip-user";
    const ADMIN_ID: &str = "s2a-admin-user";
    const LIST_A_ID: &str = "s2a-list-a-user";
    const LIST_B_ID: &str = "s2a-list-b-user";

    fn clear_users_table() {
        let txn = TREE.in_disk.begin_write().expect("begin write for clear");
        {
            let table = txn.open_table(USERS).expect("open USERS for clear");
            let keys: Vec<String> = table
                .iter()
                .expect("iterate")
                .filter_map(|r| r.ok().map(|(k, _)| k.value().to_string()))
                .collect();
            drop(table);
            let mut table = txn.open_table(USERS).expect("reopen USERS for clear");
            for key in &keys {
                table.remove(key.as_str()).expect("remove key");
            }
        }
        txn.commit().expect("commit clear");
    }

    #[test]
    fn create_and_get_user_round_trip() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_users_table();

        crate::auth::users::create_user(ROUNDTRIP_ID, true).expect("create user");
        let found = crate::auth::users::get_user(ROUNDTRIP_ID).expect("get user");
        assert_eq!(
            found,
            Some(UserRecord {
                admin: true,
                sessions: vec![]
            })
        );

        clear_users_table();
    }

    #[test]
    fn get_unknown_user_returns_none() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_users_table();

        let found = crate::auth::users::get_user("s2a-no-such-user").expect("get user");
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
        let found = crate::auth::users::get_user(ADMIN_ID).expect("get user");
        assert_eq!(
            found,
            Some(UserRecord {
                admin: true,
                sessions: vec![]
            })
        );
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
        assert!(crate::auth::users::list_users().expect("list").is_empty());

        crate::auth::users::create_user(LIST_B_ID, false).expect("create b");
        crate::auth::users::create_user(LIST_A_ID, true).expect("create a");
        assert_eq!(crate::auth::users::user_count().expect("count"), 2);
        let listed = crate::auth::users::list_users().expect("list");
        assert_eq!(
            listed,
            vec![
                (
                    LIST_A_ID.to_string(),
                    UserRecord {
                        admin: true,
                        sessions: vec![]
                    }
                ),
                (
                    LIST_B_ID.to_string(),
                    UserRecord {
                        admin: false,
                        sessions: vec![]
                    }
                ),
            ]
        );

        clear_users_table();
    }

    #[test]
    fn passwd_file_path_lives_under_data_home() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        let path = crate::auth::users::passwd_file_path();
        assert_eq!(path.file_name().expect("file name"), "passwd.json");
        assert_eq!(
            path.parent()
                .expect("parent")
                .file_name()
                .expect("parent name"),
            "auth"
        );
        assert!(path.starts_with(crate::storage::files::get_data_path()));
    }

    #[test]
    fn users_table_uses_json_string_values() {
        // The on-disk table name is `users`; values are JSON strings like ASSET_BY_ID.
        let def: TableDefinition<'static, &'static str, &'static str> = USERS;
        assert_eq!(def.to_string(), "users<&str, &str>");
    }

    #[test]
    fn provision_user_rejects_short_and_empty_password() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();

        // 7 chars is below the minimum; "password" is blocklisted; "" and
        // whitespace trim to nothing.
        for pw in ["short1!", "password", "", "   "] {
            let err = super::provision_user("s3-short", pw, false)
                .expect_err("weak/empty password must be rejected");
            assert_eq!(err.kind, crate::error::ErrorKind::InvalidInput);
        }
        // Rejected provisions leave no user record behind.
        assert_eq!(super::user_count().expect("count"), 0);
        assert!(super::get_user("s3-short").expect("get").is_none());

        reset_backend_state();
    }

    #[test]
    fn provision_user_accepts_exactly_12_chars() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();

        super::provision_user("s3-exact", "123456789012", false).expect("12 chars ok");
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
        super::provision_user("s3-trivial", "pw", false).expect("trivial accepted");
        // …and fails with it off (flag restored before asserting).
        set_trivial_passwords(false);
        let err = super::provision_user("s3-strict", "pw", false).expect_err("strict rejects");
        assert_eq!(err.kind, crate::error::ErrorKind::InvalidInput);
        // Empty stays rejected either way: presence is not strength.
        set_trivial_passwords(true);
        let err = super::provision_user("s3-empty", "   ", false).expect_err("empty rejected");
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

        super::provision_user("s3-dup", "long-enough-pw1", true).expect("first provision");
        let err = super::provision_user("s3-dup", "long-enough-pw2", false)
            .expect_err("duplicate must fail");
        assert_eq!(err.kind, crate::error::ErrorKind::Conflict);
        // The existing record is untouched by the failed duplicate.
        assert_eq!(
            super::get_user("s3-dup").expect("get"),
            Some(UserRecord {
                admin: true,
                sessions: vec![]
            })
        );

        reset_backend_state();
    }

    #[test]
    fn provision_user_trims_id_and_password_for_login_consistency() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        reset_backend_state();

        let id = super::provision_user("  s3-trim  ", "  spaced-pw-12chars  ", false)
            .expect("provision");
        assert_eq!(id, "s3-trim");
        assert_eq!(
            super::get_user("s3-trim").expect("get"),
            Some(UserRecord {
                admin: false,
                sessions: vec![]
            })
        );
        // Login verifies the trimmed input against the stored (trimmed)
        // canonical form — write-then-login must stay consistent.
        let store =
            crate::auth::password::PasswdFile::load(&super::passwd_file_path()).expect("load");
        assert!(store.verify("s3-trim", "spaced-pw-12chars"));

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
        let found = crate::auth::users::get_user("s5-sole").expect("get user");
        assert_eq!(
            found,
            Some(UserRecord {
                admin: true,
                sessions: vec![]
            })
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
        assert_eq!(
            crate::auth::users::get_user("s5-a").expect("get a"),
            Some(UserRecord {
                admin: false,
                sessions: vec![]
            })
        );
        assert_eq!(
            crate::auth::users::get_user("s5-b").expect("get b"),
            Some(UserRecord {
                admin: true,
                sessions: vec![]
            })
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
        assert_eq!(
            crate::auth::users::get_user("s5-c").expect("get c"),
            Some(UserRecord {
                admin: true,
                sessions: vec![]
            })
        );
        // Same-flag write is a no-op reporting no change.
        assert!(!super::set_admin_role("s5-c", true).expect("noop promote"));
        crate::auth::users::create_user("s5-d", false).expect("create d");
        assert!(!super::set_admin_role("s5-d", false).expect("noop demote"));
        clear_users_table();
    }

    #[test]
    fn legacy_row_reads_as_empty_sessions() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_users_table();

        // Write a legacy row without the `sessions` field directly via redb,
        // then verify `get_user` defaults it to empty.
        let txn = TREE.in_disk.begin_write().expect("begin write");
        {
            let mut table = txn.open_table(USERS).expect("open USERS");
            table
                .insert("s6-legacy", r#"{"admin":true}"#)
                .expect("insert legacy row");
        }
        txn.commit().expect("commit");

        let found = super::get_user("s6-legacy").expect("get legacy");
        assert_eq!(
            found,
            Some(UserRecord {
                admin: true,
                sessions: vec![]
            })
        );

        clear_users_table();
    }

    #[test]
    fn add_and_remove_session_round_trip() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_users_table();

        crate::auth::users::create_user("s6-sess-a", false).expect("create");
        super::add_session("s6-sess-a", "jti-1").expect("add");
        let rec = super::get_user("s6-sess-a").expect("get").expect("exists");
        assert_eq!(rec.sessions.len(), 1);
        assert_eq!(rec.sessions[0].jti, "jti-1");

        super::add_session("s6-sess-a", "jti-2").expect("add second");
        let rec = super::get_user("s6-sess-a").expect("get").expect("exists");
        assert_eq!(rec.sessions.len(), 2);
        assert!(super::remove_session("s6-sess-a", "jti-1").expect("remove"));
        let rec = super::get_user("s6-sess-a").expect("get").expect("exists");
        assert_eq!(rec.sessions.len(), 1);
        assert_eq!(rec.sessions[0].jti, "jti-2");

        clear_users_table();
    }

    #[test]
    fn remove_session_is_idempotent() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_users_table();

        crate::auth::users::create_user("s6-sess-b", false).expect("create");
        super::add_session("s6-sess-b", "jti-1").expect("add");
        assert!(super::remove_session("s6-sess-b", "jti-1").expect("first remove"));
        assert!(!super::remove_session("s6-sess-b", "jti-1").expect("second remove"));
        assert!(!super::remove_session("s6-sess-b", "jti-missing").expect("missing jti"));
        assert!(!super::remove_session("s6-unknown", "jti-1").expect("unknown user"));

        clear_users_table();
    }

    #[test]
    fn clear_sessions_removes_all() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_users_table();

        crate::auth::users::create_user("s6-sess-c", false).expect("create");
        super::add_session("s6-sess-c", "jti-1").expect("add 1");
        super::add_session("s6-sess-c", "jti-2").expect("add 2");
        super::clear_sessions("s6-sess-c").expect("clear");
        let rec = super::get_user("s6-sess-c").expect("get").expect("exists");
        assert!(rec.sessions.is_empty());
        let err = super::clear_sessions("s6-ghost").expect_err("unknown user must fail");
        assert_eq!(err.kind, crate::error::ErrorKind::NotFound);
        let err = super::add_session("s6-ghost", "jti-x").expect_err("unknown user must fail");
        assert_eq!(err.kind, crate::error::ErrorKind::NotFound);

        clear_users_table();
    }

    #[test]
    fn readd_session_refreshes_and_moves_to_back() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_users_table();

        crate::auth::users::create_user("s6-sess-d", false).expect("create");
        super::add_session("s6-sess-d", "jti-1").expect("add 1");
        // Capture first issued_at.
        let first = super::get_user("s6-sess-d")
            .expect("get")
            .expect("exists")
            .sessions[0]
            .issued_at;
        std::thread::sleep(std::time::Duration::from_secs(1));
        super::add_session("s6-sess-d", "jti-2").expect("add 2");
        // Re-add jti-1: should refresh issued_at and move to back.
        super::add_session("s6-sess-d", "jti-1").expect("re-add 1");
        let rec = super::get_user("s6-sess-d").expect("get").expect("exists");
        assert_eq!(rec.sessions.len(), 2);
        assert_eq!(rec.sessions[0].jti, "jti-2");
        assert_eq!(rec.sessions[1].jti, "jti-1");
        assert!(rec.sessions[1].issued_at >= first);
        assert!(rec.sessions[1].issued_at >= rec.sessions[0].issued_at);

        clear_users_table();
    }

    #[test]
    fn add_session_prunes_expired() {
        let _guard = TEST_SERIAL_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = &*TEST_ENV;
        clear_users_table();

        crate::auth::users::create_user("s6-sess-e", false).expect("create");
        // Inject an expired session directly via redb.
        let expired_at = {
            let now = (chrono::Utc::now().timestamp_millis() / 1000) as u64;
            now.saturating_sub(super::SESSION_TTL_SECS + 10)
        };
        let stale = UserRecord {
            admin: false,
            sessions: vec![
                super::SessionRecord {
                    jti: "old".to_string(),
                    issued_at: expired_at,
                },
                super::SessionRecord {
                    jti: "fresh".to_string(),
                    issued_at: (chrono::Utc::now().timestamp_millis() / 1000) as u64,
                },
            ],
        };
        let txn = TREE.in_disk.begin_write().expect("begin write");
        {
            let mut table = txn.open_table(USERS).expect("open USERS");
            let value = serde_json::to_string(&stale).expect("serialize stale");
            table
                .insert("s6-sess-e", value.as_str())
                .expect("insert stale");
        }
        txn.commit().expect("commit stale");

        // Adding a new session must lazily prune the expired one.
        super::add_session("s6-sess-e", "jti-new").expect("add new");
        let rec = super::get_user("s6-sess-e").expect("get").expect("exists");
        let jtis: Vec<_> = rec.sessions.iter().map(|s| s.jti.as_str()).collect();
        assert!(!jtis.contains(&"old"), "expired session must be pruned");
        assert!(jtis.contains(&"fresh"));
        assert!(jtis.contains(&"jti-new"));
        assert_eq!(rec.sessions.len(), 2);

        clear_users_table();
    }
}
