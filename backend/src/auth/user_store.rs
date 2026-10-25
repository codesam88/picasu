#![allow(unsafe_code, clippy::missing_errors_doc, clippy::must_use_candidate)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use redb::{ReadableDatabase as _, ReadableTable as _};
use serde::{Deserialize, Serialize};

use crate::auth::password::{PasswordHash, hash_password, restrict_permissions, verify_password};
use crate::error::{AppError, ErrorKind};

/// The identity record: the password hash fields plus the identity
/// attributes, flattened into one row. Pre-change (hash-only) entries still
/// deserialize because `admin`, `home` and `ver` default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserRecord {
    #[serde(flatten)]
    #[serde(default)]
    pub hash: Option<PasswordHash>,
    #[serde(default)]
    pub admin: bool,
    #[serde(default)]
    pub home: Option<String>,
    /// Token version. Every token carries the version it was minted with;
    /// the guard rejects a token whose version is not the current one. Bumped
    /// by every change that must invalidate the outstanding tokens (password,
    /// role) and by an explicit invalidation.
    #[serde(default)]
    pub ver: u64,
}

/// The single table of the user store: user id to the JSON encoding of
/// [`UserRecord`]. The value is JSON text, like every other table in the
/// project — one encoding rule, no byte/text exception.
const USERS_TABLE: redb::TableDefinition<&str, &str> = redb::TableDefinition::new("users");

/// Surfaced when the store file is locked by another process. The server
/// holds the file for its whole lifetime, so the offline CLI (or any
/// second process) must run while the server is stopped.
const STORE_LOCKED_MESSAGE: &str = "user store is held by another process (the server may be running); stop the server before running this offline command";

/// Process-wide cache of open redb handles, keyed by path. Opening the
/// same path twice in one process races into `DatabaseAlreadyOpen`, so
/// every backend entry point shares one `Arc<Database>` per path. Handles
/// are never evicted for the process lifetime: dropping the `Database`
/// releases the OS file lock, and the held lock is what excludes writers
/// in other processes. [`remove_store`] is the only eviction path.
static REDB_HANDLES: LazyLock<Mutex<HashMap<PathBuf, Arc<redb::Database>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn map_open_error(error: redb::DatabaseError, path: &Path) -> AppError {
    if matches!(error, redb::DatabaseError::DatabaseAlreadyOpen) {
        AppError::new(
            ErrorKind::Conflict,
            format!("{STORE_LOCKED_MESSAGE}: {}", path.display()),
        )
    } else {
        AppError::from_err(ErrorKind::IO, error.into())
    }
}

fn open_db(path: &Path) -> Result<Arc<redb::Database>, AppError> {
    // The whole check-create-insert runs under the cache lock so two
    // threads racing first use share one handle instead of one of them
    // tripping `DatabaseAlreadyOpen` against the other.
    let mut handles = REDB_HANDLES.lock().expect("redb handle cache poisoned");
    if let Some(db) = handles.get(path) {
        return Ok(Arc::clone(db));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::from_err(ErrorKind::IO, e.into()))?;
    }
    let db = Arc::new(redb::Database::create(path).map_err(|e| map_open_error(e, path))?);
    restrict_permissions(path);
    handles.insert(path.to_path_buf(), Arc::clone(&db));
    Ok(db)
}

/// Map an arbitrary error to the store's IO error kind.
fn io(error: impl Into<anyhow::Error>) -> AppError {
    AppError::from_err(ErrorKind::IO, error.into())
}

/// Decode collected `(id, raw)` row pairs. Any undecodable value fails
/// the whole load: the store fail-closes (readers report empty, `load`
/// reports an error) instead of serving partial state.
fn decode_pairs(pairs: Vec<(String, String)>) -> anyhow::Result<HashMap<String, UserRecord>> {
    let mut users = HashMap::with_capacity(pairs.len());
    for (id, raw) in pairs {
        let user: UserRecord = serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("corrupt user entry {id:?}: {e}"))?;
        users.insert(id, user);
    }
    Ok(users)
}

fn load_users(path: &Path) -> Result<HashMap<String, UserRecord>, AppError> {
    // Readers never materialize the file: a missing path is an empty
    // store, and the boot gate relies on `exists()` meaning "provisioned".
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let db = open_db(path)?;
    let txn = db.begin_read().map_err(io)?;
    let table = match txn.open_table(USERS_TABLE) {
        Ok(table) => table,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(HashMap::new()),
        Err(e) => return Err(io(e)),
    };
    let pairs: Vec<(String, String)> = table
        .iter()
        .map_err(io)?
        .map(|entry| {
            entry
                .map_err(io)
                .map(|(key, value)| (key.value().to_owned(), value.value().to_owned()))
        })
        .collect::<Result<_, _>>()?;
    decode_pairs(pairs).map_err(io)
}

/// Locked read-modify-write: one write transaction owns the whole
/// load-mutate-persist cycle. The closure runs against state read inside
/// the transaction; when it returns `Err` the transaction is dropped
/// without commit, publishing nothing. Commit publishes atomically.
pub fn rmw<T, F>(path: &Path, f: F) -> Result<T, AppError>
where
    F: FnOnce(&mut HashMap<String, UserRecord>) -> Result<T, AppError>,
{
    fn io(error: impl Into<anyhow::Error>) -> AppError {
        AppError::from_err(ErrorKind::IO, error.into())
    }
    let db = open_db(path)?;
    let write = db.begin_write().map_err(io)?;
    let mut table = write.open_table(USERS_TABLE).map_err(io)?;
    // Rows are collected into owned pairs first so no table borrow is
    // alive when the closure runs and the table is mutated below.
    let pairs: Vec<(String, String)> = table
        .iter()
        .map_err(io)?
        .map(|entry| {
            entry
                .map_err(io)
                .map(|(key, value)| (key.value().to_owned(), value.value().to_owned()))
        })
        .collect::<Result<_, _>>()?;
    let mut users = decode_pairs(pairs).map_err(io)?;
    let result = f(&mut users)?;
    for (id, user) in &users {
        let raw = serde_json::to_string(user).map_err(io)?;
        table.insert(id.as_str(), raw.as_str()).map_err(io)?;
    }
    // 4.3.0 has no table clear/drain: remove keys the closure deleted.
    let stale: Vec<String> = table
        .iter()
        .map_err(io)?
        .map(|entry| {
            entry.map_err(io).map(|(key, _)| {
                let id = key.value().to_owned();
                (!users.contains_key(&id)).then_some(id)
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?
        .into_iter()
        .flatten()
        .collect();
    for id in &stale {
        table.remove(id.as_str()).map_err(io)?;
    }
    drop(table);
    write.commit().map_err(io)?;
    Ok(result)
}

pub fn provision(
    path: &Path,
    id: &str,
    password: &str,
    admin: bool,
    home: Option<String>,
) -> Result<String, AppError> {
    let id = crate::auth::users::validate_user_id(id)?;
    let canonical = crate::auth::users::validate_new_password(&id, password)?;
    rmw(path, |users| {
        if users.contains_key(&id) {
            return Err(AppError::new(ErrorKind::Conflict, "user already exists"));
        }
        users.insert(
            id.clone(),
            UserRecord {
                hash: Some(hash_password(&canonical)),
                admin,
                home,
                ver: 0,
            },
        );
        Ok(id)
    })
}

/// Write a new password hash for `id` and invalidate the user's outstanding
/// tokens. The version bump happens in the same transaction as the hash
/// write: a crash between the two would leave the old tokens working
/// against a new password.
pub fn set_password(path: &Path, id: &str, password: &str) -> Result<(), AppError> {
    let id = id.trim().to_string();
    let canonical = password.trim().to_string();
    let hash = hash_password(&canonical);
    rmw(path, |users| {
        if let Some(entry) = users.get_mut(&id) {
            entry.hash = Some(hash.clone());
            entry.ver = entry.ver.wrapping_add(1);
        } else {
            users.insert(
                id.clone(),
                UserRecord {
                    hash: Some(hash.clone()),
                    admin: false,
                    home: None,
                    ver: 1,
                },
            );
        }
        Ok(())
    })
}

/// Set the admin flag of an existing user and invalidate the user's
/// outstanding tokens: the role lives in the token, so the new role takes
/// effect at the next sign-in. Returns `Ok(false)` when the flag already
/// had that value — a no-op must not kill the user's sessions.
pub fn set_admin(path: &Path, id: &str, admin: bool) -> Result<bool, AppError> {
    let target = id.trim().to_string();
    rmw(path, |users| {
        let current = users
            .get(&target)
            .cloned()
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, format!("unknown user: {target}")))?;
        if current.admin == admin {
            return Ok(false);
        }
        if !admin {
            let admins = users.values().filter(|e| e.admin).count();
            if admins <= 1 {
                return Err(AppError::new(
                    ErrorKind::Conflict,
                    "demotion would leave zero admins",
                ));
            }
        }
        let mut current = current;
        current.admin = admin;
        current.ver = current.ver.wrapping_add(1);
        users.insert(target, current);
        Ok(true)
    })
}

/// Invalidate every outstanding token of `id` by bumping its version. The
/// user keeps its password, role and attributes; only the tokens die.
///
/// # Errors
/// Returns [`ErrorKind::NotFound`] when the id is unknown.
pub fn bump_ver(path: &Path, id: &str) -> Result<u64, AppError> {
    let target = id.trim().to_string();
    rmw(path, |users| {
        let current = users
            .get(&target)
            .cloned()
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, format!("unknown user: {target}")))?;
        let mut current = current;
        current.ver = current.ver.wrapping_add(1);
        let ver = current.ver;
        users.insert(target, current);
        Ok(ver)
    })
}

/// Set the stored home directory for an existing user, preserving the
/// password hash and admin flag. `None` clears the attribute.
pub fn set_home(path: &Path, id: &str, home: Option<String>) -> Result<bool, AppError> {
    let target = id.trim().to_string();
    rmw(path, |users| {
        let current = users
            .get(&target)
            .cloned()
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, format!("unknown user: {target}")))?;
        if current.home == home {
            return Ok(false);
        }
        let mut updated = current;
        updated.home = home;
        users.insert(target, updated);
        Ok(true)
    })
}

pub fn delete(path: &Path, id: &str, force: bool) -> Result<bool, AppError> {
    let target = id.trim().to_string();
    rmw(path, |users| {
        let current = match users.get(&target) {
            Some(e) => e.clone(),
            None => return Ok(false),
        };
        if current.admin && !force {
            let admins = users.values().filter(|e| e.admin).count();
            if admins <= 1 {
                return Err(AppError::new(
                    ErrorKind::Conflict,
                    "deletion would leave zero admins",
                ));
            }
        }
        users.remove(&target);
        Ok(true)
    })
}

// Lockless readers.
pub fn read(path: &Path, id: &str) -> Option<UserRecord> {
    load_users(path).ok()?.get(id.trim()).cloned()
}

/// Read one entry, reporting why the store could not be read. [`read`]
/// swallows that error, which makes an unreadable store look like a
/// missing entry — an offline admin tool must not confuse the two.
pub fn read_checked(path: &Path, id: &str) -> Result<Option<UserRecord>, AppError> {
    Ok(load_users(path)?.get(id.trim()).cloned())
}

pub fn list(path: &Path) -> Vec<(String, UserRecord)> {
    let mut v: Vec<_> = load_users(path)
        .ok()
        .unwrap_or_default()
        .into_iter()
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// List every entry, reporting why the store could not be read. [`list`]
/// swallows that error, which makes an unreadable store look like an empty
/// one — an offline admin tool must not confuse the two.
pub fn list_checked(path: &Path) -> Result<Vec<(String, UserRecord)>, AppError> {
    let mut v: Vec<_> = load_users(path)?.into_iter().collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(v)
}

pub fn count(path: &Path) -> usize {
    load_users(path).map_or(0, |u| u.len())
}

pub fn is_empty(path: &Path) -> bool {
    count(path) == 0
}

/// Close the cached handle for `path` and delete the store file. Test
/// resets use this because the process-lifetime handle would otherwise
/// keep serving the deleted file's contents. An operator deletes the
/// store file directly only while the server is stopped.
///
/// # Panics
/// Panics if the handle cache mutex is poisoned.
pub fn remove_store(path: &Path) {
    let mut handles = REDB_HANDLES.lock().expect("redb handle cache poisoned");
    handles.remove(path);
    drop(handles);
    let _ = std::fs::remove_file(path);
}

/// The user store at an explicit file path: a dedicated redb file holding
/// one table of user id to the JSON encoding of [`UserRecord`]
/// (`{hash?, admin, home}`; a hash-less entry cannot log in). The struct
/// is an in-memory snapshot view; its methods write through the free
/// functions above.
#[derive(Debug)]
pub struct UserStore {
    path: PathBuf,
    users: HashMap<String, UserRecord>,
}

impl UserStore {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let users = load_users(path).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(Self {
            path: path.to_path_buf(),
            users,
        })
    }

    pub fn set_password(&mut self, user_id: &str, password: &str) -> anyhow::Result<()> {
        set_password(&self.path, user_id, password).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.users = load_users(&self.path).unwrap_or_default();
        Ok(())
    }

    pub fn verify(&self, user_id: &str, password: &str) -> bool {
        self.users
            .get(user_id)
            .is_some_and(|stored| match &stored.hash {
                Some(hash) => verify_password(password, hash),
                None => false,
            })
    }

    pub fn is_empty(&self) -> bool {
        self.users.is_empty()
    }

    // Convenience locked ops using self.path.
    pub fn provision(
        &self,
        id: &str,
        password: &str,
        admin: bool,
        home: Option<String>,
    ) -> Result<String, AppError> {
        provision(&self.path, id, password, admin, home)
    }
    pub fn set_admin(&self, id: &str, admin: bool) -> Result<bool, AppError> {
        set_admin(&self.path, id, admin)
    }
    pub fn delete(&self, id: &str) -> Result<bool, AppError> {
        delete(&self.path, id, false)
    }
    pub fn read_entry(&self, id: &str) -> Option<UserRecord> {
        read(&self.path, id)
    }
    pub fn list_entries(&self) -> Vec<(String, UserRecord)> {
        list(&self.path)
    }
    pub fn count_entries(&self) -> usize {
        count(&self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::fixtures::test_password;

    fn store_path(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("users.redb")
    }

    /// The token version of a stored user.
    fn ver_of(path: &Path, id: &str) -> u64 {
        read(path, id).expect("entry must exist").ver
    }

    /// Substrate + integrity assertion: every store test calls this after
    /// mutations. Reopens the file through the engine and decodes all
    /// rows, so a green suite proves the store is a parsable redb
    /// database holding well-formed entries.
    fn assert_store_parses(path: &Path) {
        let db = open_db(path).expect("store file must be a redb database");
        let txn = db.begin_read().expect("begin read");
        let table = txn.open_table(USERS_TABLE).expect("users table");
        decode_pairs(
            table
                .iter()
                .expect("iterate")
                .map(|entry| {
                    let (key, value) = entry.expect("row");
                    (key.value().to_owned(), value.value().to_owned())
                })
                .collect(),
        )
        .expect("decode rows");
    }

    #[test]
    fn provision_carries_admin_home() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(
            &path,
            "alice",
            &test_password("alice"),
            true,
            Some("/home/alice".to_string()),
        )
        .expect("provision");
        let entry = read(&path, "alice").expect("entry");
        assert!(entry.admin, "admin flag must survive provisioning");
        assert_eq!(entry.home.as_deref(), Some("/home/alice"));
        assert!(entry.hash.is_some(), "provisioned entry must carry a hash");
        assert_store_parses(&path);
    }

    #[test]
    fn raw_value_is_json_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(&path, "alice", &test_password("alice"), false, None).expect("provision");
        let db = open_db(&path).expect("open store");
        let txn = db.begin_read().expect("begin read");
        let table = txn.open_table(USERS_TABLE).expect("open users table");
        let guard = table.get("alice").expect("get").expect("alice row");
        let value: serde_json::Value =
            serde_json::from_str(guard.value()).expect("value decodes as JSON");
        assert!(value.get("salt").is_some(), "salt must be present");
        assert!(value.get("hash").is_some(), "hash must be present");
        assert!(
            value.get("iterations").is_some(),
            "iterations must be present"
        );
    }

    #[test]
    fn set_password_preserves_admin_home() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(
            &path,
            "alice",
            &test_password("alice"),
            true,
            Some("/home/alice".to_string()),
        )
        .expect("provision");
        set_password(&path, "alice", &test_password("alice-new")).expect("set password");
        let entry = read(&path, "alice").expect("entry");
        assert!(entry.admin, "admin must be preserved");
        assert_eq!(entry.home.as_deref(), Some("/home/alice"));
        let store = UserStore::load(&path).expect("reload");
        assert!(store.verify("alice", &test_password("alice-new")));
        assert!(!store.verify("alice", &test_password("alice")));
        assert_store_parses(&path);
    }

    #[test]
    fn zero_admin_demotion_refused_flag_untouched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(&path, "sole", &test_password("sole"), true, None).expect("provision");
        let err = set_admin(&path, "sole", false).expect_err("sole admin demotion must be refused");
        assert_eq!(err.kind, ErrorKind::Conflict);
        let entry = read(&path, "sole").expect("entry");
        assert!(entry.admin, "flag must be untouched after refusal");
        assert_store_parses(&path);
    }

    #[test]
    fn delete_zero_admin_refusal_and_force() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(&path, "sole", &test_password("sole"), true, None).expect("provision");
        let err =
            delete(&path, "sole", false).expect_err("deleting the last admin must be refused");
        assert_eq!(err.kind, ErrorKind::Conflict);
        assert!(read(&path, "sole").is_some(), "entry must survive refusal");
        assert!(delete(&path, "sole", true).expect("force delete"));
        assert!(read(&path, "sole").is_none(), "force must remove the entry");
        assert!(is_empty(&path));
        assert_store_parses(&path);
    }

    #[test]
    fn delete_unknown_returns_false() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(&path, "alice", &test_password("alice"), false, None).expect("provision");
        assert!(!delete(&path, "nobody", false).expect("delete unknown"));
        assert_store_parses(&path);
    }

    #[test]
    fn set_home_preserves_and_noop() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(
            &path,
            "alice",
            &test_password("alice"),
            true,
            Some("/home/alice".to_string()),
        )
        .expect("provision");
        assert!(
            !set_home(&path, "alice", Some("/home/alice".to_string())).expect("no-op set_home"),
            "setting the same home must report no change"
        );
        assert!(
            set_home(&path, "alice", Some("/home/elsewhere".to_string())).expect("change home")
        );
        let entry = read(&path, "alice").expect("entry");
        assert_eq!(entry.home.as_deref(), Some("/home/elsewhere"));
        assert!(entry.admin, "admin must be preserved");
        assert!(entry.hash.is_some(), "hash must be preserved");
        assert!(
            set_home(&path, "alice", None).expect("clear home"),
            "clearing must report a change"
        );
        assert!(read(&path, "alice").expect("entry").home.is_none());
        assert_store_parses(&path);
    }

    #[test]
    fn set_admin_noop_false() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(&path, "alice", &test_password("alice"), false, None).expect("provision");
        assert!(
            !set_admin(&path, "alice", false).expect("no-op demote"),
            "setting the same flag must report no change"
        );
        assert!(set_admin(&path, "alice", true).expect("promote"));
        assert!(read(&path, "alice").expect("entry").admin);
        assert_store_parses(&path);
    }

    #[test]
    fn provision_duplicate_conflict() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(&path, "alice", &test_password("alice"), false, None).expect("provision");
        let err = provision(&path, "alice", &test_password("alice-2"), false, None)
            .expect_err("duplicate provision must fail");
        assert_eq!(err.kind, ErrorKind::Conflict);
        assert_eq!(count(&path), 1, "failed provision must not add a row");
        assert_store_parses(&path);
    }

    #[test]
    fn unknown_user_read_and_updates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(&path, "alice", &test_password("alice"), false, None).expect("provision");
        assert!(read(&path, "nobody").is_none());
        let err = set_admin(&path, "nobody", true).expect_err("unknown set_admin");
        assert_eq!(err.kind, ErrorKind::NotFound);
        let err = set_home(&path, "nobody", None).expect_err("unknown set_home");
        assert_eq!(err.kind, ErrorKind::NotFound);
        assert_store_parses(&path);
    }

    #[test]
    fn passwd_file_verify_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(&path, "alice", &test_password("alice"), false, None).expect("provision");
        let store = UserStore::load(&path).expect("load");
        assert!(store.verify("alice", &test_password("alice")));
        assert!(!store.verify("alice", &test_password("alice-wrong")));
        assert!(!store.verify("nobody", &test_password("alice")));
        assert_store_parses(&path);
    }

    #[test]
    fn missing_path_loads_empty_without_creating() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        assert!(!path.exists(), "precondition");
        let store = UserStore::load(&path).expect("missing file loads as empty");
        assert!(store.is_empty(), "missing file loads as an empty store");
        assert!(read(&path, "alice").is_none());
        assert!(list(&path).is_empty(), "fresh store lists no users");
        assert_eq!(count(&path), 0);
        assert!(is_empty(&path));
        assert!(
            !path.exists(),
            "readers must not materialize the store file"
        );
    }

    #[test]
    fn remove_store_deletes_and_recreates_fresh() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(&path, "alice", &test_password("alice"), false, None).expect("provision");
        remove_store(&path);
        assert!(!path.exists(), "remove_store must delete the file");
        assert!(is_empty(&path), "store must read as empty after reset");
        provision(&path, "bob", &test_password("bob"), false, None).expect("reprovision");
        assert_eq!(count(&path), 1, "fresh store after reset");
        assert!(read(&path, "bob").is_some());
        assert_store_parses(&path);
    }

    #[test]
    fn corrupt_store_fails_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        std::fs::write(&path, b"not a redb file").expect("write corrupt file");
        assert!(
            UserStore::load(&path).is_err(),
            "load must report an error on a corrupt store"
        );
        assert!(read(&path, "alice").is_none(), "read fails closed");
        assert!(list(&path).is_empty(), "list fails closed");
        assert_eq!(count(&path), 0, "count fails closed");
    }

    #[test]
    fn concurrent_distinct_key_provisions_both_land() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        let path1 = path.clone();
        let path2 = path.clone();
        let t1 = std::thread::spawn(move || {
            provision(&path1, "user-a", &test_password("user-a"), false, None)
                .expect("provision a");
        });
        let t2 = std::thread::spawn(move || {
            provision(&path2, "user-b", &test_password("user-b"), false, None)
                .expect("provision b");
        });
        t1.join().expect("t1");
        t2.join().expect("t2");
        let entries = list(&path);
        let ids: Vec<_> = entries.iter().map(|(k, _)| k.clone()).collect();
        assert!(ids.contains(&"user-a".to_string()), "user-a must land");
        assert!(ids.contains(&"user-b".to_string()), "user-b must land");
        assert_eq!(entries.len(), 2);
        assert_store_parses(&path);
    }

    #[test]
    fn concurrent_field_edits_both_persist() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(&path, "alice", &test_password("alice"), false, None).expect("provision");
        let path1 = path.clone();
        let path2 = path.clone();
        let t1 = std::thread::spawn(move || {
            set_admin(&path1, "alice", true).expect("promote");
        });
        let t2 = std::thread::spawn(move || {
            set_home(&path2, "alice", Some("/home/alice".to_string())).expect("set home");
        });
        t1.join().expect("t1");
        t2.join().expect("t2");
        let entry = read(&path, "alice").expect("entry");
        assert!(entry.admin, "concurrent promote must persist");
        assert_eq!(entry.home.as_deref(), Some("/home/alice"));
        assert_store_parses(&path);
    }

    #[test]
    fn hash_none_entry_never_verifies() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        rmw(&path, |users| {
            users.insert(
                "ghost".to_string(),
                UserRecord {
                    hash: None,
                    admin: false,
                    home: None,
                    ver: 0,
                },
            );
            Ok(())
        })
        .expect("insert hash-less entry");
        let store = UserStore::load(&path).expect("load");
        assert!(!store.verify("ghost", &test_password("ghost")));
        assert!(!store.verify("ghost", ""));
        assert_store_parses(&path);
    }

    /// Child-process marker for the two cross-process tests below: the child
    /// is a re-exec of this test binary running a single test with
    /// `PICASU_STORE_TEST_CHILD` set, and takes the child branch here.
    fn child_mode() -> Option<String> {
        std::env::var("PICASU_STORE_TEST_CHILD").ok()
    }

    fn child_path() -> PathBuf {
        PathBuf::from(std::env::var("PICASU_STORE_TEST_PATH").expect("child store path"))
    }

    #[test]
    fn cross_process_lock_pin() {
        const MODE: &str = "lock-pin";
        const THIS_TEST: &str = "auth::user_store::tests::cross_process_lock_pin";
        if child_mode().as_deref() == Some(MODE) {
            match provision(
                &child_path(),
                "child-user",
                &test_password("child-user"),
                false,
                None,
            ) {
                Ok(_) => std::process::exit(10),
                Err(e) => {
                    if e.message.contains("held by another process") {
                        std::process::exit(0);
                    }
                    eprintln!("unexpected child error: {e}");
                    std::process::exit(11);
                }
            }
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(&path, "owner", &test_password("owner"), true, None).expect("parent provision");
        assert_store_parses(&path);
        let exe = std::env::current_exe().expect("current test binary");
        let status = std::process::Command::new(&exe)
            .arg("--exact")
            .arg(THIS_TEST)
            .arg("--nocapture")
            .env("PICASU_STORE_TEST_CHILD", MODE)
            .env("PICASU_STORE_TEST_PATH", &path)
            .status()
            .expect("spawn child");
        assert!(
            status.success(),
            "child mutation must fail loudly with the held-by-another-process message, got {status}"
        );
        assert_eq!(count(&path), 1, "parent store must be intact");
    }

    #[test]
    fn kill_during_write_leaves_parsable_store() {
        use std::os::unix::process::ExitStatusExt as _;
        const MODE: &str = "kill-writer";
        const THIS_TEST: &str = "auth::user_store::tests::kill_during_write_leaves_parsable_store";
        if child_mode().as_deref() == Some(MODE) {
            let path = child_path();
            let ready =
                PathBuf::from(std::env::var("PICASU_STORE_TEST_READY").expect("child ready path"));
            provision(&path, "pre-user", &test_password("pre-user"), true, None).expect("seed");
            // Park inside an uncommitted write transaction holding partial
            // rows: the parent SIGKILLs us here, and the engine must drop
            // the transaction so the file stays at the committed state.
            let db = open_db(&path).expect("open");
            let write = db.begin_write().expect("begin write");
            {
                let mut table = write.open_table(USERS_TABLE).expect("open table");
                for i in 0..8 {
                    let id = format!("torn-row-{i}");
                    let raw = serde_json::to_string(&UserRecord {
                        hash: Some(hash_password(&test_password("torn-row"))),
                        admin: false,
                        home: None,
                        ver: 0,
                    })
                    .expect("encode");
                    table.insert(id.as_str(), raw.as_str()).expect("insert");
                }
                std::fs::write(&ready, b"ready").expect("signal ready");
                loop {
                    std::thread::sleep(std::time::Duration::from_hours(1));
                }
            }
        }
        let dir = tempfile::tempdir().expect("tempdir");
        // Fresh path the parent never touches before the kill: the lock must
        // be free for the child to write.
        let path = dir.path().join("kill.redb");
        let ready = dir.path().join("ready");
        let exe = std::env::current_exe().expect("current test binary");
        let mut child = std::process::Command::new(&exe)
            .arg("--exact")
            .arg(THIS_TEST)
            .arg("--nocapture")
            .env("PICASU_STORE_TEST_CHILD", MODE)
            .env("PICASU_STORE_TEST_PATH", &path)
            .env("PICASU_STORE_TEST_READY", &ready)
            .spawn()
            .expect("spawn child");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !ready.exists() {
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                panic!("child never reached the parked write transaction");
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let pid = child.id().cast_signed();
        let rc = unsafe { libc::kill(pid, libc::SIGKILL) };
        assert_eq!(rc, 0, "SIGKILL must succeed");
        let status = child.wait().expect("reap child");
        assert_eq!(
            status.signal(),
            Some(libc::SIGKILL),
            "child must die by SIGKILL, got {status}"
        );
        // The store must be parsable at a prefix-consistent state: the
        // committed seed is present and valid, the uncommitted rows are gone.
        let store = UserStore::load(&path).expect("store must parse after kill");
        assert!(store.verify("pre-user", &test_password("pre-user")));
        assert!(!store.verify("pre-user", &test_password("pre-user-wrong")));
        let entries = list(&path);
        assert_eq!(
            entries.len(),
            1,
            "only the committed seed may be visible, got {:?}",
            entries.iter().map(|(k, _)| k).collect::<Vec<_>>()
        );
        assert_eq!(entries[0].0, "pre-user");
        assert_store_parses(&path);
    }

    // ── token version ──────────────────────────────────────────────────────

    #[test]
    fn a_new_user_starts_at_version_zero() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(&path, "alice", &test_password("alice"), true, None).expect("provision");
        assert_eq!(ver_of(&path, "alice"), 0);
    }

    #[test]
    fn changing_the_password_bumps_the_version() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(&path, "alice", &test_password("alice"), true, None).expect("provision");
        set_password(&path, "alice", &test_password("alice-new")).expect("set password");
        assert_eq!(
            ver_of(&path, "alice"),
            1,
            "a new password must invalidate the outstanding tokens"
        );
    }

    #[test]
    fn changing_the_role_bumps_the_version() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(&path, "alice", &test_password("alice"), false, None).expect("provision");
        provision(&path, "root", &test_password("root"), true, None).expect("provision");
        set_admin(&path, "alice", true).expect("promote");
        assert_eq!(ver_of(&path, "alice"), 1);
        set_admin(&path, "alice", false).expect("demote");
        assert_eq!(ver_of(&path, "alice"), 2);
    }

    #[test]
    fn setting_the_home_keeps_the_version() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(&path, "alice", &test_password("alice"), true, None).expect("provision");
        set_home(&path, "alice", Some("/home/alice".to_string())).expect("set home");
        assert_eq!(
            ver_of(&path, "alice"),
            0,
            "the home attribute carries no authority and must not kill tokens"
        );
    }

    #[test]
    fn bumping_the_version_invalidates_the_outstanding_tokens() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        provision(&path, "alice", &test_password("alice"), true, None).expect("provision");
        assert_eq!(bump_ver(&path, "alice").expect("bump"), 1);
        assert_eq!(ver_of(&path, "alice"), 1);
        assert_eq!(bump_ver(&path, "alice").expect("bump"), 2);
        assert_eq!(ver_of(&path, "alice"), 2);
    }

    #[test]
    fn bumping_the_version_of_an_unknown_user_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = store_path(&dir);
        let err = bump_ver(&path, "nobody").expect_err("unknown user must fail");
        assert_eq!(err.kind, ErrorKind::NotFound);
    }
}
