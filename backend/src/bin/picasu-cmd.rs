//! Offline user administration CLI for Picasu.
//!
//! The passwd store under `PICASU_DATA_HOME` is the userdb of record; this
//! binary reuses the same locked store primitives as the server, so offline
//! edits stay coherent with online ones. Session state lives in the index
//! redb database and is purged on `user del` (see the ordering note on
//! [`cmd_del`]).

use std::io::{BufRead, IsTerminal as _, Write as _};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Offline user administration for the Picasu photo gallery.
#[derive(Parser, Debug)]
#[command(
    name = "picasu-cmd",
    version,
    about = "Offline administration CLI for Picasu"
)]
struct Cli {
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
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("picasu-cmd: error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    let TopLevel::User { command } = cli.command;
    match command {
        UserCommand::List { user } => cmd_list(user),
        UserCommand::Add {
            user,
            admin,
            home,
            password,
        } => cmd_add(&user, admin, home, password),
        UserCommand::Mod {
            user,
            home,
            admin,
            no_admin,
        } => cmd_mod(&user, home, admin, no_admin),
        UserCommand::Del { user, force } => cmd_del(&user, force),
        UserCommand::Passwd { user, password } => cmd_passwd(&user, password),
    }
}

fn cmd_list(user: Option<String>) -> Result<(), String> {
    use picasu::auth::password::UserEntry;
    let path = picasu::auth::users::user_store_path();
    let rows: Vec<(String, UserEntry)> = match user {
        Some(id) => {
            let id = canonical_id(&id)?;
            match picasu::auth::passwd_store::read(&path, &id) {
                Some(entry) => vec![(id, entry)],
                None => return Err(format!("unknown user: {id}")),
            }
        }
        None => picasu::auth::passwd_store::list(&path),
    };
    print_table(&rows);
    Ok(())
}

fn print_table(rows: &[(String, picasu::auth::password::UserEntry)]) {
    println!("{:<16} {:<5} {:<20} PASSWORD", "ID", "ADMIN", "HOME");
    for (id, entry) in rows {
        let admin = if entry.admin { "y" } else { "n" };
        let home = entry.home.as_deref().unwrap_or("-");
        let password = if entry.hash.is_some() { "set" } else { "none" };
        println!("{id:<16} {admin:<5} {home:<20} {password}");
    }
}

fn cmd_add(
    user: &str,
    admin: bool,
    home: Option<String>,
    password: Option<String>,
) -> Result<(), String> {
    let path = picasu::auth::users::user_store_path();
    let password = match password {
        Some(password) => password,
        None => prompt_twice()?,
    };
    // `provision` validates the id and the password policy, hashes, and
    // inserts under the store lock; duplicates are a conflict error.
    let id = picasu::auth::passwd_store::provision(&path, user, &password, admin, home)
        .map_err(|e| e.to_string())?;
    println!("created user '{id}'");
    Ok(())
}

fn cmd_mod(user: &str, home: Option<String>, admin: bool, no_admin: bool) -> Result<(), String> {
    let path = picasu::auth::users::user_store_path();
    let id = canonical_id(user)?;
    if picasu::auth::passwd_store::read(&path, &id).is_none() {
        return Err(format!("unknown user: {id}"));
    }
    let mut changed = false;
    if let Some(home) = home {
        changed |= picasu::auth::passwd_store::set_home(&path, &id, Some(home))
            .map_err(|e| e.to_string())?;
    }
    if admin {
        changed |=
            picasu::auth::passwd_store::set_admin(&path, &id, true).map_err(|e| e.to_string())?;
    } else if no_admin {
        // The zero-admin guard lives inside the locked transaction, so a
        // concurrent demotion cannot slip through.
        changed |=
            picasu::auth::passwd_store::set_admin(&path, &id, false).map_err(|e| e.to_string())?;
    }
    if changed {
        println!("updated user '{id}'");
    } else {
        println!("no changes requested for '{id}'");
    }
    Ok(())
}

fn cmd_del(user: &str, force: bool) -> Result<(), String> {
    let path = picasu::auth::users::user_store_path();
    let id = canonical_id(user)?;
    let Some(current) = picasu::auth::passwd_store::read(&path, &id) else {
        return Err(format!("unknown user: {id}"));
    };
    if current.admin && !force {
        let other_admin = picasu::auth::passwd_store::list(&path)
            .into_iter()
            .any(|(uid, entry)| uid != id && entry.admin);
        if !other_admin {
            return Err(format!(
                "refusing to delete the last admin '{id}'; use --force to override"
            ));
        }
    }
    // The store delete runs before the session purge: a store failure must
    // not leave sessions pointing at a deleted user, so abort first. The
    // residual ordering consequence is the reverse — if the purge itself
    // fails after the delete, a stale session row remains. That row is
    // harmless: authentication reads the passwd entry, which is already
    // gone, so the orphaned sessions can never verify.
    let removed =
        picasu::auth::passwd_store::delete(&path, &id, force).map_err(|e| e.to_string())?;
    if !removed {
        return Err(format!("unknown user: {id}"));
    }
    picasu::auth::users::purge_sessions(&id)
        .map_err(|e| format!("deleted user '{id}' but failed to purge sessions: {e}"))?;
    println!("deleted user '{id}'");
    Ok(())
}

fn cmd_passwd(user: &str, password: Option<String>) -> Result<(), String> {
    let path = picasu::auth::users::user_store_path();
    let id = canonical_id(user)?;
    if picasu::auth::passwd_store::read(&path, &id).is_none() {
        return Err(format!("unknown user: {id}"));
    }
    let password = match password {
        Some(password) => password,
        None => prompt_twice()?,
    };
    // Same policy the server enforces; `set_password` only swaps the hash,
    // preserving `admin` and `home`.
    let canonical =
        picasu::auth::users::validate_new_password(&id, &password).map_err(|e| e.to_string())?;
    picasu::auth::passwd_store::set_password(&path, &id, &canonical).map_err(|e| e.to_string())?;
    println!("updated password for '{id}'");
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
