//! Integration tests for the `picasu-cmd` offline admin CLI.
//!
//! Each test owns a fresh temp dir wired through `PICASU_DATA_HOME`, so the
//! suite is serial-safe by construction: tests spawn the compiled binary as
//! separate processes and never share a store. `PICASU_WEAK_PWHASH=1`
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
        .find_map(std::env::var_os)
        .map(PathBuf::from)
        .expect("cargo must expose the picasu-cmd binary path to integration tests")
}

/// Environment for a CLI invocation. The location variables are optional:
/// which one the CLI picks is part of what the tests pin.
#[derive(Default)]
struct Env {
    data_home: Option<PathBuf>,
    config_home: Option<PathBuf>,
    vars: Vec<(String, String)>,
}

impl Env {
    fn data_home(home: &Path) -> Self {
        Self {
            data_home: Some(home.to_path_buf()),
            ..Self::default()
        }
    }
}

/// Run the CLI with an explicit environment. The location variables are
/// removed first, so a value inherited from the developer's shell cannot
/// decide the outcome.
fn run_env(env: &Env, args: &[&str], stdin: Option<&str>) -> Output {
    let mut child = std::process::Command::new(bin_path());
    child
        .args(args)
        .env_remove("PICASU_DATA_HOME")
        .env_remove("PICASU_CONFIG_HOME")
        .env_remove("PICASU_TRIVIAL_PASSWORDS")
        .env("PICASU_WEAK_PWHASH", "1");
    if let Some(home) = &env.data_home {
        child.env("PICASU_DATA_HOME", home);
    }
    if let Some(home) = &env.config_home {
        child.env("PICASU_CONFIG_HOME", home);
    }
    for (key, value) in &env.vars {
        child.env(key, value);
    }
    child
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

fn run(data_home: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    run_env(&Env::data_home(data_home), args, stdin)
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
    let row = table.get(id).expect("read row")?;
    Some(serde_json::from_str(row.value()).expect("entry must decode as JSON"))
}

/// The store table as the CLI writes it: user id to JSON-encoded entry.
/// Redeclared here because the definition lives in the lib-private store
/// module (same pattern as `users_table` below for the index DB).
fn store_table() -> redb::TableDefinition<'static, &'static str, &'static str> {
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

// Pin 4: `user del` refuses the last admin, `--force` removes the entry.
#[test]
fn pin4_user_del_guards_last_admin() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        &["user", "add", "alice", "--password", ALICE_PW, "--admin"],
        None,
    );
    assert!(out.status.success());

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

// Pin 7: `--userdb` names the store file, and the CLI reports the file it
// resolved.
#[test]
fn pin7_userdb_flag_selects_the_store_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = dir.path().join("elsewhere").join("users.redb");
    let out = run_env(
        &Env::default(),
        &[
            "--userdb",
            store.to_str().expect("utf-8 path"),
            "user",
            "add",
            "alice",
            "--password",
            ALICE_PW,
            "--admin",
        ],
        None,
    );
    assert!(
        out.status.success(),
        "add must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        store.exists(),
        "the store must be created at the given path, not elsewhere"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("users.redb"),
        "the CLI must report the store it works on, stderr: {stderr}"
    );
    assert!(
        stderr.contains(&store.display().to_string()),
        "the CLI must report where it creates the store, stderr: {stderr}"
    );
}

// Pin 8: `PICASU_DATA_HOME` puts the store under the data home's `auth/`.
#[test]
fn pin8_data_home_env_selects_the_store_under_the_auth_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run_env(
        &Env::data_home(dir.path()),
        &["user", "add", "alice", "--password", ALICE_PW, "--admin"],
        None,
    );
    assert!(
        out.status.success(),
        "add must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let store = store_path(dir.path());
    assert!(store.exists(), "the store must live under the data home");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("users.redb"),
        "the CLI must report the store it works on, stderr: {stderr}"
    );
    assert!(
        stderr.contains(&store.display().to_string()),
        "the CLI must report where it creates the store, stderr: {stderr}"
    );
}

// Pin 9: the server's `config.toml` names the data home, so the CLI finds
// the store the server uses — even without the environment variable.
#[test]
fn pin9_config_file_data_home_selects_the_store() {
    let config_home = tempfile::tempdir().expect("tempdir");
    let data_home = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        config_home.path().join("config.toml"),
        format!(
            "[gallery]\ndata_home = {:?}\n",
            data_home.path().to_string_lossy()
        ),
    )
    .expect("write config.toml");
    let env = Env {
        config_home: Some(config_home.path().to_path_buf()),
        ..Env::default()
    };
    let out = run_env(
        &env,
        &["user", "add", "alice", "--password", ALICE_PW, "--admin"],
        None,
    );
    assert!(
        out.status.success(),
        "add must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let store = store_path(data_home.path());
    assert!(
        store.exists(),
        "the store must land in the configured data home"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("users.redb"),
        "the CLI must report the store it works on, stderr: {stderr}"
    );
    assert!(
        stderr.contains(&store.display().to_string()),
        "the CLI must report where it creates the store, stderr: {stderr}"
    );
}

// Pin 10: without a location the CLI refuses to run — it must not guess
// one, and it must not create a store somewhere arbitrary.
#[test]
fn pin10_without_a_location_the_cli_refuses_to_run() {
    let config_home = tempfile::tempdir().expect("tempdir");
    let env = Env {
        config_home: Some(config_home.path().to_path_buf()),
        ..Env::default()
    };
    let out = run_env(
        &env,
        &["user", "add", "alice", "--password", ALICE_PW],
        None,
    );
    assert!(
        !out.status.success(),
        "add must fail without a location, stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("cannot determine the user store location"),
        "the refusal must name the missing location, stderr: {stderr}"
    );
    assert!(
        stderr.contains("PICASU_DATA_HOME") && stderr.contains("--userdb"),
        "the refusal must name both location options, stderr: {stderr}"
    );
}

// Pin 11: a read command on a missing store fails and says where it
// looked — an empty table would read as "no users exist".
#[test]
fn pin11_list_without_a_store_fails_and_names_the_location() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(dir.path(), &["user", "list"], None);
    assert!(
        !out.status.success(),
        "list must fail without a store, stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("could not find users.redb"),
        "the failure must name the store it looked for, stderr: {stderr}"
    );
    assert!(
        stderr.contains("set location via PICASU_DATA_HOME or --userdb"),
        "the failure must name both location options, stderr: {stderr}"
    );
    assert!(
        !stderr.contains(&store_path(dir.path()).display().to_string()),
        "the failure must not name a path that does not exist, stderr: {stderr}"
    );
}

// Pin 12: `user add` creates the store when it is missing — the first
// admin is provisioned before the server has ever run.
#[test]
fn pin12_add_creates_a_missing_store_at_the_resolved_location() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        &["user", "add", "alice", "--password", ALICE_PW],
        None,
    );
    assert!(
        out.status.success(),
        "add must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        store_path(dir.path()).exists(),
        "add must create the store at the resolved location"
    );
}

// Pin 13: the CLI mirrors the server's password policy: `PICASU_TRIVIAL_PASSWORDS`
// switches the strength checks off, exactly as on the server.
#[test]
fn pin13_trivial_passwords_env_mirrors_the_server_policy() {
    const WEAK_PW: &str = "short";

    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        &["user", "add", "alice", "--password", WEAK_PW],
        None,
    );
    assert!(
        !out.status.success(),
        "a weak password must be refused by default, stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    let dir2 = tempfile::tempdir().expect("tempdir");
    let env = Env {
        vars: vec![("PICASU_TRIVIAL_PASSWORDS".to_string(), "true".to_string())],
        ..Env::data_home(dir2.path())
    };
    let out = run_env(&env, &["user", "add", "alice", "--password", WEAK_PW], None);
    assert!(
        out.status.success(),
        "a weak password must be accepted with trivial passwords on, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        entry(dir2.path(), "alice").get("hash").is_some(),
        "the accepted password must be hashed into the store"
    );
}

// Pin 14: an existing store is reported with the file it works on.
#[test]
fn pin14_existing_store_is_reported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        &["user", "add", "alice", "--password", ALICE_PW, "--admin"],
        None,
    );
    assert!(out.status.success());
    let out = run(dir.path(), &["user", "list"], None);
    assert!(
        out.status.success(),
        "list must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    let store = store_path(dir.path());
    assert!(
        stderr.contains(&format!("Using users.redb at {}", store.display())),
        "the CLI must report the store it works on, stderr: {stderr}"
    );
}

// Pin 15: a store held by another process fails loudly. An empty table
// would claim there are no users — the one reading it would be wrong.
#[test]
fn pin15_locked_store_fails_loudly_instead_of_reporting_an_empty_store() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        &["user", "add", "alice", "--password", ALICE_PW],
        None,
    );
    assert!(out.status.success());

    let holder = redb::Database::create(store_path(dir.path())).expect("hold the store open");
    let out = run(dir.path(), &["user", "list"], None);
    assert!(
        !out.status.success(),
        "list must fail while the store is held, stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("held by another process"),
        "the failure must name the conflict, stderr: {stderr}"
    );
    drop(holder);
}

// Pin 16: a password change, a role change and an explicit invalidation
// bump the user's token version — the outstanding tokens die with it.
#[test]
fn pin16_password_role_and_invalidate_bump_the_token_version() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        &["user", "add", "alice", "--password", ALICE_PW, "--admin"],
        None,
    );
    assert!(out.status.success());
    assert_eq!(version(dir.path(), "alice"), 0);

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
    assert_eq!(version(dir.path(), "alice"), 1);

    // A second admin, so the demotion of the first one is allowed.
    let out = run(
        dir.path(),
        &["user", "add", "bob", "--password", BOB_PW, "--admin"],
        None,
    );
    assert!(out.status.success());
    let out = run(dir.path(), &["user", "mod", "alice", "--no-admin"], None);
    assert!(
        out.status.success(),
        "mod must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(version(dir.path(), "alice"), 2);

    let out = run(dir.path(), &["user", "invalidate", "alice"], None);
    assert!(
        out.status.success(),
        "invalidate must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(version(dir.path(), "alice"), 3);
}

/// The token version stored for a user.
fn version(data_home: &Path, id: &str) -> u64 {
    entry(data_home, id)
        .get("ver")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default()
}
