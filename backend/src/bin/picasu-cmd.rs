//! Offline user administration CLI for Picasu.
//!
//! The passwd store under the data home is the userdb of record; this
//! binary reuses the same locked store primitives as the server, so offline
//! edits stay coherent with online ones. Session state lives in the index
//! redb database and is purged on `user del` (see the ordering note on
//! [`cmd_del`]).

use std::io::{BufRead, IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use picasu::auth::store_location::{self, UserStoreLocation};

/// Where the CLI looks for the user store, and how to redirect it.
const LOCATION_HELP: &str = "\
User store location:
  The user store is the file <data home>/auth/users.redb. It is resolved in
  this order:

    1. --userdb <PATH>                    the store file itself
    2. $PICASU_DATA_HOME                  <data home>/auth/users.redb
    3. config.toml ([gallery] data_home)  <data home>/auth/users.redb

  With none of them set the CLI does not know where the store is and
  refuses to run. $PICASU_DATA_HOME also tells the server where to expect
  the store: it writes the same data_home into its config.toml on first
  launch.";

/// No location could be resolved: the CLI must not guess one.
const LOCATION_UNKNOWN: &str =
    "cannot determine the user store location; set location via PICASU_DATA_HOME or --userdb";

/// Offline administration for the Picasu photo gallery.
#[derive(Parser, Debug)]
#[command(
    name = "picasu-cmd",
    version,
    about = "Offline administration CLI for Picasu",
    after_help = LOCATION_HELP
)]
struct Cli {
    /// Path to the user store file (…/auth/users.redb). Overrides
    /// `PICASU_DATA_HOME` and the `data_home` in config.toml.
    #[arg(long, global = true, value_name = "PATH")]
    userdb: Option<PathBuf>,
    #[command(subcommand)]
    command: TopLevel,
}

#[derive(Subcommand, Debug)]
enum TopLevel {
    /// Inspect and edit users in the passwd store.
    User {
        #[command(subcommand)]
        command: UserCommand,
    },
}

#[derive(Subcommand, Debug)]
enum UserCommand {
    /// List users: all entries, or a single entry by id.
    List {
        /// Show only this user. Fails when the id is unknown.
        user: Option<String>,
    },
    /// Create a user. Without `--password`, prompts twice with no echo.
    Add {
        /// New user id (server-side id policy applies).
        user: String,
        /// Grant the admin flag.
        #[arg(long)]
        admin: bool,
        /// Stored home directory (opaque to the server for now).
        #[arg(long)]
        home: Option<String>,
        /// Password on the command line (testing plumbing). When absent,
        /// the password is prompted interactively, twice.
        #[arg(long)]
        password: Option<String>,
    },
    /// Edit attributes. Only the flags given change anything.
    Mod {
        /// Existing user id.
        user: String,
        /// Replace the stored home directory.
        #[arg(long)]
        home: Option<String>,
        /// Grant the admin flag.
        #[arg(long, conflicts_with = "no_admin")]
        admin: bool,
        /// Revoke the admin flag. Refused on the last admin.
        #[arg(long)]
        no_admin: bool,
    },
    /// Delete a user and purge its sessions. Refuses the last admin
    /// unless `--force` is given.
    Del {
        /// Existing user id.
        user: String,
        /// Also delete the last remaining admin.
        #[arg(long)]
        force: bool,
    },
    /// Re-key a user's password, preserving admin and home. Without
    /// `--password`, prompts twice with no echo.
    Passwd {
        /// Existing user id.
        user: String,
        /// Password on the command line (testing plumbing). When absent,
        /// the password is prompted interactively, twice.
        #[arg(long)]
        password: Option<String>,
    },
    /// Invalidate every outstanding token of a user. The password, role and
    /// attributes stay; the sessions die.
    Invalidate {
        /// Existing user id.
        user: String,
    },
}

fn main() -> ExitCode {
    init_logger();
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("picasu-cmd: error: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Warnings the library emits (weakened hashing, trivial passwords) must
/// reach the admin: there is no server log around the CLI to carry them.
fn init_logger() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .write_style(env_logger::WriteStyle::Auto)
        .init();
}

/// Mirror the server's password policy: the `PICASU_TRIVIAL_PASSWORDS`
/// environment variable wins over the `trivial_passwords` key in
/// `config.toml`, and an unparsable value keeps the config value. The
/// PBKDF2 effort needs no setup here — the library reads
/// `PICASU_PBKDF_ITERATIONS` when it hashes.
fn apply_password_policy() {
    let from_env = std::env::var("PICASU_TRIVIAL_PASSWORDS")
        .ok()
        .and_then(|value| value.trim().parse::<bool>().ok());
    let enabled = from_env
        .or(picasu::configured_trivial_passwords())
        .unwrap_or(false);
    picasu::auth::users::set_trivial_passwords_override(enabled);
}

fn run(cli: Cli) -> Result<(), String> {
    apply_password_policy();
    let location = locate(&cli)?;
    let TopLevel::User { command } = cli.command;
    match command {
        UserCommand::List { user } => cmd_list(&location, user),
        UserCommand::Add {
            user,
            admin,
            home,
            password,
        } => cmd_add(&location, &user, admin, home, password),
        UserCommand::Mod {
            user,
            home,
            admin,
            no_admin,
        } => cmd_mod(&location, &user, home, admin, no_admin),
        UserCommand::Del { user, force } => cmd_del(&location, &user, force),
        UserCommand::Passwd { user, password } => cmd_passwd(&location, &user, password),
        UserCommand::Invalidate { user } => cmd_invalidate(&location, &user),
    }
}

/// Resolve the store the command works on. The CLI touches one file: the
/// identity record.
fn locate(cli: &Cli) -> Result<UserStoreLocation, String> {
    let location = store_location::locate_user_store(cli.userdb.as_deref())
        .ok_or_else(|| LOCATION_UNKNOWN.to_string())?;
    if location.path.is_dir() {
        return Err(format!(
            "user store path is a directory, not a file: {}",
            location.path.display()
        ));
    }
    Ok(location)
}

/// Report the store the command works on. Read commands fail when the file
/// is missing: an empty result would read as "no users exist".
fn store_for_read(location: &UserStoreLocation) -> Result<PathBuf, String> {
    if location.path.exists() {
        report_using(&location.path);
        Ok(location.path.clone())
    } else {
        Err(not_found(&location.path))
    }
}

/// Report the store the command works on. Write commands create the store
/// when it is missing: `user add` provisions the first admin before the
/// server has ever run.
fn store_for_write(location: &UserStoreLocation) -> PathBuf {
    if location.path.exists() {
        report_using(&location.path);
    } else {
        eprintln!("{}", not_found(&location.path));
        eprintln!("creating a new store at {}", location.path.display());
    }
    location.path.clone()
}

fn report_using(path: &Path) {
    eprintln!(
        "Using {} at {}",
        store_location::USER_STORE_FILE_NAME,
        path.display()
    );
}

fn not_found(path: &Path) -> String {
    format!(
        "could not find {} at {}; set location via PICASU_DATA_HOME or --userdb",
        store_location::USER_STORE_FILE_NAME,
        path.display()
    )
}

fn cmd_list(location: &UserStoreLocation, user: Option<String>) -> Result<(), String> {
    use picasu::auth::user_store::UserRecord;
    let path = store_for_read(location)?;
    let rows: Vec<(String, UserRecord)> = match user {
        Some(id) => {
            let id = canonical_id(&id)?;
            match picasu::auth::user_store::read_checked(&path, &id).map_err(|e| e.to_string())? {
                Some(entry) => vec![(id, entry)],
                None => return Err(format!("unknown user: {id}")),
            }
        }
        None => picasu::auth::user_store::list_checked(&path).map_err(|e| e.to_string())?,
    };
    print_table(&rows);
    Ok(())
}

fn print_table(rows: &[(String, picasu::auth::user_store::UserRecord)]) {
    println!("{:<16} {:<5} {:<20} PASSWORD", "ID", "ADMIN", "HOME");
    for (id, entry) in rows {
        let admin = if entry.admin { "y" } else { "n" };
        let home = entry.home.as_deref().unwrap_or("-");
        let password = if entry.hash.is_some() { "set" } else { "none" };
        println!("{id:<16} {admin:<5} {home:<20} {password}");
    }
}

fn cmd_add(
    location: &UserStoreLocation,
    user: &str,
    admin: bool,
    home: Option<String>,
    password: Option<String>,
) -> Result<(), String> {
    let path = store_for_write(location);
    let password = match password {
        Some(password) => password,
        None => prompt_twice()?,
    };
    // `provision` validates the id and the password policy, hashes, and
    // inserts under the store lock; duplicates are a conflict error.
    let id = picasu::auth::user_store::provision(&path, user, &password, admin, home)
        .map_err(|e| e.to_string())?;
    println!("created user '{id}'");
    Ok(())
}

fn cmd_mod(
    location: &UserStoreLocation,
    user: &str,
    home: Option<String>,
    admin: bool,
    no_admin: bool,
) -> Result<(), String> {
    let path = store_for_read(location)?;
    let id = canonical_id(user)?;
    if picasu::auth::user_store::read_checked(&path, &id)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        return Err(format!("unknown user: {id}"));
    }
    let mut changed = false;
    if let Some(home) = home {
        changed |= picasu::auth::user_store::set_home(&path, &id, Some(home))
            .map_err(|e| e.to_string())?;
    }
    if admin {
        changed |=
            picasu::auth::user_store::set_admin(&path, &id, true).map_err(|e| e.to_string())?;
    } else if no_admin {
        // The zero-admin guard lives inside the locked transaction, so a
        // concurrent demotion cannot slip through.
        changed |=
            picasu::auth::user_store::set_admin(&path, &id, false).map_err(|e| e.to_string())?;
    }
    if changed {
        println!("updated user '{id}'");
    } else {
        println!("no changes requested for '{id}'");
    }
    Ok(())
}

fn cmd_del(location: &UserStoreLocation, user: &str, force: bool) -> Result<(), String> {
    let path = store_for_read(location)?;
    let id = canonical_id(user)?;
    let Some(current) =
        picasu::auth::user_store::read_checked(&path, &id).map_err(|e| e.to_string())?
    else {
        return Err(format!("unknown user: {id}"));
    };
    if current.admin && !force {
        let other_admin = picasu::auth::user_store::list(&path)
            .into_iter()
            .any(|(uid, entry)| uid != id && entry.admin);
        if !other_admin {
            return Err(format!(
                "refusing to delete the last admin '{id}'; use --force to override"
            ));
        }
    }
    // Deleting the identity record is enough: the server keeps no session
    // list, and a token for a user that no longer exists is refused by the
    // guard.
    let removed = picasu::auth::user_store::delete(&path, &id, force).map_err(|e| e.to_string())?;
    if !removed {
        return Err(format!("unknown user: {id}"));
    }
    println!("deleted user '{id}'");
    Ok(())
}

fn cmd_passwd(
    location: &UserStoreLocation,
    user: &str,
    password: Option<String>,
) -> Result<(), String> {
    let path = store_for_read(location)?;
    let id = canonical_id(user)?;
    if picasu::auth::user_store::read_checked(&path, &id)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        return Err(format!("unknown user: {id}"));
    }
    let password = match password {
        Some(password) => password,
        None => prompt_twice()?,
    };
    // Same policy the server enforces; `set_password` only swaps the hash,
    // preserving `admin` and `home`, and bumps the token version so the
    // outstanding tokens of that user die with the old password.
    let canonical =
        picasu::auth::users::validate_new_password(&id, &password).map_err(|e| e.to_string())?;
    picasu::auth::user_store::set_password(&path, &id, &canonical).map_err(|e| e.to_string())?;
    println!("updated password for '{id}'");
    Ok(())
}

/// Invalidate every outstanding token of a user. The password, role and
/// attributes stay; the sessions die.
fn cmd_invalidate(location: &UserStoreLocation, user: &str) -> Result<(), String> {
    let path = store_for_read(location)?;
    let id = canonical_id(user)?;
    let ver = picasu::auth::user_store::bump_ver(&path, &id).map_err(|e| e.to_string())?;
    println!("invalidated the tokens of '{id}' (version {ver})");
    Ok(())
}

/// Validate the id up front so every subcommand reports malformed ids the
/// same way (the store trims too, but only validation rejects empties).
fn canonical_id(user: &str) -> Result<String, String> {
    picasu::auth::users::validate_user_id(user).map_err(|e| e.to_string())
}

/// Read a password twice and require an exact match. Nothing is written
/// before both reads agree, so a mismatch never leaves a half-created
/// entry behind.
fn prompt_twice() -> Result<String, String> {
    if std::io::stdin().is_terminal() {
        // Real terminal: hidden input with no echo.
        let first = rpassword::prompt_password("New password: ")
            .map_err(|e| format!("failed to read password: {e}"))?;
        let second = rpassword::prompt_password("Retype new password: ")
            .map_err(|e| format!("failed to read password: {e}"))?;
        if first != second {
            return Err("passwords do not match".to_string());
        }
        return Ok(first);
    }
    // Piped stdin (scripts, tests, CI): there is no /dev/tty to read from,
    // so consume lines directly. One lock across both reads — a fresh
    // buffer per line could swallow the second line through read-ahead.
    // Prompts go to stderr to keep stdout machine-readable.
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let first = read_piped_line(&mut input, "New password: ")?;
    let second = read_piped_line(&mut input, "Retype new password: ")?;
    if first != second {
        return Err("passwords do not match".to_string());
    }
    Ok(first)
}

fn read_piped_line(input: &mut impl BufRead, prompt: &str) -> Result<String, String> {
    eprint!("{prompt}");
    std::io::stderr()
        .flush()
        .map_err(|e| format!("failed to write prompt: {e}"))?;
    let mut line = String::new();
    let bytes = input
        .read_line(&mut line)
        .map_err(|e| format!("failed to read password: {e}"))?;
    if bytes == 0 {
        return Err("unexpected end of input while reading password".to_string());
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}
