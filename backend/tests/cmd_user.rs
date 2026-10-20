//! Integration tests for the `picasu-cmd` offline admin CLI.
//!
//! Each test owns a fresh temp dir wired through `PICASU_DATA_HOME`, so the
//! suite is serial-safe by construction: tests spawn the compiled binary as
//! separate processes and never share a store. `PICASU_PBKDF_ITERATIONS=1000`
//! keeps hashing off the dev-loop critical path, mirroring `just
//! backend-test`.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};

use redb::ReadableDatabase as _;

/// `picasu-cmd` binary under test. Cargo exposes the absolute path to
/// integration tests under the verbatim target name (verified: dashes are
/// NOT converted on this toolchain), with an underscore fallback in case
/// that layout ever changes.
fn bin_path() -> PathBuf {
    ["CARGO_BIN_EXE_picasu_cmd", "CARGO_BIN_EXE_picasu-cmd"]
        .into_iter()
        .find_map(|key| std::env::var_os(key))
        .map(PathBuf::from)
        .expect("cargo must expose the picasu-cmd binary path to integration tests")
}

fn run(data_home: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut child = std::process::Command::new(bin_path());
    child
        .args(args)
        .env("PICASU_DATA_HOME", data_home)
        .env("PICASU_PBKDF_ITERATIONS", "1000")
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = child.spawn().expect("spawn picasu-cmd");
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(input.as_bytes())
            .expect("write piped stdin");
    }
    child.wait_with_output().expect("wait for picasu-cmd")
}

fn store_path(data_home: &Path) -> PathBuf {
    data_home.join("auth").join("users.redb")
}

/// Read one user's raw JSON entry out of the store file. Opens, reads,
/// and drops (never through the lib's store API, which caches a
/// process-lifetime handle and would hold the file lock across subprocess
/// boundaries and lock out the CLI under test).
fn raw_entry(data_home: &Path, id: &str) -> Option<serde_json::Value> {
    let path = store_path(data_home);
    if !path.exists() {
        return None;
    }
    let db = redb::Database::open(&path).expect("store must be a readable redb file");
    let txn = db.begin_read().expect("begin read");
    let table = match txn.open_table(store_table()) {
        Ok(table) => table,
        Err(redb::TableError::TableDoesNotExist(_)) => return None,
        Err(e) => panic!("open users table: {e}"),
    };
    let Some(row) = table.get(id).expect("read row") else {
        return None;
    };
    Some(serde_json::from_slice(row.value()).expect("entry must decode as JSON"))
}

/// The store table as the CLI writes it: user id to JSON-encoded entry.
/// Redeclared here because the definition lives in the lib-private store
/// module (same pattern as `users_table` below for the index DB).
fn store_table() -> redb::TableDefinition<'static, &'static str, &'static [u8]> {
    redb::TableDefinition::new("users")
}

fn entry(data_home: &Path, id: &str) -> serde_json::Value {
    raw_entry(data_home, id).unwrap_or_else(|| panic!("store must contain user {id}"))
}

const ALICE_PW: &str = "long-enough-pw1";
const ALICE_PW2: &str = "long-enough-pw2";
const BOB_PW: &str = "another-long-pw3";

// Pin 1: `user add` creates the entry with admin flag and password hash.
#[test]
fn pin1_user_add_creates_admin_entry_with_hash() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        &["user", "add", "alice", "--password", ALICE_PW, "--admin"],
        None,
    );
    assert!(
        out.status.success(),
        "add must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let alice = entry(dir.path(), "alice");
    assert_eq!(alice.get("admin"), Some(&serde_json::Value::Bool(true)));
    assert!(
        alice.get("hash").is_some_and(|h| !h.is_null()),
        "password hash must be present, got: {alice}"
    );
}

// Pin 2: `user mod` edits home, preserves hash/admin; last-admin demotion
// is refused.
#[test]
fn pin2_user_mod_home_and_last_admin_guard() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        &["user", "add", "alice", "--password", ALICE_PW, "--admin"],
        None,
    );
    assert!(out.status.success());

    let out = run(
        dir.path(),
        &["user", "mod", "alice", "--home", "/srv/img/alice"],
        None,
    );
    assert!(
        out.status.success(),
        "mod --home must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let alice = entry(dir.path(), "alice");
    assert_eq!(
        alice.get("home"),
        Some(&serde_json::json!("/srv/img/alice"))
    );
    assert_eq!(alice.get("admin"), Some(&serde_json::Value::Bool(true)));
    assert!(alice.get("hash").is_some_and(|h| !h.is_null()));

    let out = run(dir.path(), &["user", "mod", "alice", "--no-admin"], None);
    assert!(
        !out.status.success(),
        "demoting the last admin must fail, stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(
        entry(dir.path(), "alice").get("admin"),
        Some(&serde_json::Value::Bool(true)),
        "refused demotion must leave the flag untouched"
    );
}

// Pin 3: interactive double-prompt over piped stdin; mismatch aborts.
#[test]
fn pin3_interactive_add_prompts_twice_over_piped_stdin() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        &["user", "add", "bob"],
        Some(&format!("{BOB_PW}\n{BOB_PW}\n")),
    );
    assert!(
        out.status.success(),
        "matching prompts must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bob = entry(dir.path(), "bob");
    assert!(bob.get("hash").is_some_and(|h| !h.is_null()));

    let dir2 = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir2.path(),
        &["user", "add", "carol"],
        Some("mismatched-pw-1\nmismatched-pw-2\n"),
    );
    assert!(
        !out.status.success(),
        "mismatched confirmation must fail, stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        raw_entry(dir2.path(), "carol").is_none(),
        "mismatched confirmation must not create an entry"
    );
}

// Pin 4: `user del` refuses the last admin, `--force` removes the entry and
// purges the redb session row.
#[test]
fn pin4_user_del_guards_last_admin_and_purges_sessions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        &["user", "add", "alice", "--password", ALICE_PW, "--admin"],
        None,
    );
    assert!(out.status.success());

    // Seed a live session row directly in the index DB: the CLI under test
    // is the only writer, so open/close around it and never hold the DB
    // across the subprocess boundary.
    seed_session_row(dir.path(), "alice", "jti-pin4");
    assert!(session_row_present(dir.path(), "alice"));

    let out = run(dir.path(), &["user", "del", "alice"], None);
    assert!(
        !out.status.success(),
        "deleting the last admin must be refused, stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    entry(dir.path(), "alice");

    let out = run(dir.path(), &["user", "del", "alice", "--force"], None);
    assert!(
        out.status.success(),
        "--force delete must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        raw_entry(dir.path(), "alice").is_none(),
        "forced delete must remove the entry"
    );
    assert!(
        !session_row_present(dir.path(), "alice"),
        "forced delete must purge the redb session row"
    );
}

// Pin 5: `user passwd` re-keys, preserves attributes; unknown user fails.
#[test]
fn pin5_user_passwd_rekeys_and_preserves_attributes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        &[
            "user",
            "add",
            "alice",
            "--password",
            ALICE_PW,
            "--admin",
            "--home",
            "/srv/img/alice",
        ],
        None,
    );
    assert!(out.status.success());
    let before = entry(dir.path(), "alice");

    let out = run(
        dir.path(),
        &["user", "passwd", "alice", "--password", ALICE_PW2],
        None,
    );
    assert!(
        out.status.success(),
        "passwd must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let after = entry(dir.path(), "alice");
    assert_ne!(
        before.get("hash"),
        after.get("hash"),
        "passwd must replace the hash"
    );
    assert_eq!(after.get("admin"), before.get("admin"));
    assert_eq!(after.get("home"), before.get("home"));

    let out = run(
        dir.path(),
        &["user", "passwd", "mallory", "--password", ALICE_PW2],
        None,
    );
    assert!(
        !out.status.success(),
        "passwd on an unknown user must fail, stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

// Pin 6: `user list` renders the table, a single entry, and fails cleanly
// on unknown ids.
#[test]
fn pin6_user_list_table_single_and_unknown() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        &["user", "add", "alice", "--password", ALICE_PW, "--admin"],
        None,
    );
    assert!(out.status.success());
    let out = run(
        dir.path(),
        &["user", "add", "bob", "--password", BOB_PW],
        None,
    );
    assert!(out.status.success());

    let out = run(dir.path(), &["user", "list"], None);
    assert!(
        out.status.success(),
        "list must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("alice"), "list must show alice:\n{stdout}");
    assert!(stdout.contains("bob"), "list must show bob:\n{stdout}");

    let out = run(dir.path(), &["user", "list", "alice"], None);
    assert!(
        out.status.success(),
        "list alice must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("alice"),
        "single list must show alice:\n{stdout}"
    );

    let out = run(dir.path(), &["user", "list", "mallory"], None);
    assert!(
        !out.status.success(),
        "list of an unknown user must fail, stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

// ── redb helpers (pin 4) ────────────────────────────────────────────────
//
// The `USERS` table definition lives in the lib-private `storage::db`
// module, so tests redeclare its stable shape here: on-disk name `users`
// with JSON-string values (pinned by the `users_table_uses_json_string_values`
// unit test). Rows serialize the public `UserRecord`; handles are opened,
// used, and dropped inside each helper so the CLI subprocess never
// contends for the database lock.
fn users_table() -> redb::TableDefinition<'static, &'static str, &'static str> {
    redb::TableDefinition::new("users")
}

fn index_db_path(data_home: &Path) -> PathBuf {
    data_home.join("db").join("index_v5.redb")
}

fn seed_session_row(data_home: &Path, id: &str, jti: &str) {
    let db_path = index_db_path(data_home);
    std::fs::create_dir_all(db_path.parent().expect("db parent")).expect("create db dir");
    let db = redb::Database::create(&db_path).expect("open index db for seeding");
    let record = picasu::auth::users::UserRecord {
        sessions: vec![picasu::auth::users::SessionRecord {
            jti: jti.to_string(),
            issued_at: 1_700_000_000,
        }],
    };
    let value = serde_json::to_string(&record).expect("serialize session record");
    let txn = db.begin_write().expect("begin seed write");
    {
        let mut table = txn
            .open_table(users_table())
            .expect("open USERS for seeding");
        table
            .insert(id, value.as_str())
            .expect("insert session row");
    }
    txn.commit().expect("commit seed write");
}

fn session_row_present(data_home: &Path, id: &str) -> bool {
    let db = redb::Database::open(&index_db_path(data_home)).expect("open index db for read");
    let txn = db.begin_read().expect("begin read");
    let table = txn.open_table(users_table()).expect("open USERS for read");
    table.get(id).expect("read session row").is_some()
}
