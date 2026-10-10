use anyhow::{Context, Result};
use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata};
use serde::{Deserialize, Serialize};

use crate::storage::db::{TREE, USERS};

/// A user record stored in the `USERS` redb table, keyed by user id.
/// Deliberately minimal; extended with new optional fields later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserRecord {
    pub admin: bool,
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
        let value = serde_json::to_string(&UserRecord { admin })
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
        if table.get(id)?.is_none() {
            anyhow::bail!("unknown user: {id}");
        }
        let value = serde_json::to_string(&UserRecord { admin })
            .context("Failed to serialize UserRecord")?;
        table
            .insert(id, value.as_str())
            .context("Failed to update USERS")?;
    }
    txn.commit().context("Failed to commit USERS update")
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
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        clear_users_table();

        crate::auth::users::create_user(ROUNDTRIP_ID, true).expect("create user");
        let found = crate::auth::users::get_user(ROUNDTRIP_ID).expect("get user");
        assert_eq!(found, Some(UserRecord { admin: true }));

        clear_users_table();
    }

    #[test]
    fn get_unknown_user_returns_none() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        clear_users_table();

        let found = crate::auth::users::get_user("s2a-no-such-user").expect("get user");
        assert_eq!(found, None);

        clear_users_table();
    }

    #[test]
    fn create_duplicate_user_fails() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        clear_users_table();

        crate::auth::users::create_user(ADMIN_ID, false).expect("create user");
        assert!(crate::auth::users::create_user(ADMIN_ID, true).is_err());

        clear_users_table();
    }

    #[test]
    fn set_admin_updates_flag() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let _ = &*TEST_ENV;
        clear_users_table();

        crate::auth::users::create_user(ADMIN_ID, false).expect("create user");
        crate::auth::users::set_admin(ADMIN_ID, true).expect("set admin");
        let found = crate::auth::users::get_user(ADMIN_ID).expect("get user");
        assert_eq!(found, Some(UserRecord { admin: true }));
        assert!(crate::auth::users::set_admin("s2a-no-such-user", true).is_err());

        clear_users_table();
    }

    #[test]
    fn list_users_and_count() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
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
                (LIST_A_ID.to_string(), UserRecord { admin: true }),
                (LIST_B_ID.to_string(), UserRecord { admin: false }),
            ]
        );

        clear_users_table();
    }

    #[test]
    fn passwd_file_path_lives_under_data_home() {
        let _guard = TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner());
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
}
