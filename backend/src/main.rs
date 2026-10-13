use std::path::PathBuf;

use clap::Parser;

/// Picasu — a self-hosted, filesystem-first photo gallery.
///
/// With no flags the server boots and serves the gallery. The flags below
/// are one-shot operations: they print, check, or configure, then exit
/// instead of serving. Unknown flags are rejected rather than ignored, so
/// a typo never boots a server with unintended settings.
#[derive(Parser, Debug)]
#[command(
    name = "picasu",
    version,
    about = "Self-hosted filesystem-first photo gallery"
)]
struct Cli {
    /// Print the public API document to stdout and exit.
    #[arg(long)]
    dump_openapi: bool,

    /// Verify the committed API document against the mounted routes
    /// and exit. With no value, checks `backend/openapi.json`.
    ///
    /// Three states by construction (absent / bare flag / flag with
    /// value), hence the nested option rather than a custom enum.
    #[allow(clippy::option_option)]
    #[arg(long, num_args = 0..=1, value_name = "SPEC")]
    check_openapi: Option<Option<PathBuf>>,

    /// Disable the default password-strength checks for new passwords.
    /// Development and throwaway installs only — the bypass is
    /// warn-logged at first use.
    #[arg(long)]
    trivial_passwords: bool,

    /// Enable online enrollment of the first administrator: while set and
    /// the user store is empty, unauthenticated user creation and sign-in
    /// are open. Refuses to start if a password store already exists.
    /// Equivalent to `PICASU_BOOTSTRAP_ADMIN=true`.
    #[arg(long)]
    bootstrap_admin: bool,
}

fn main() {
    let cli = Cli::parse();

    // A sibling of `--dump-openapi`, and the only reason the committed spec is
    // read at all: the route-set gate runs when asked for, never at boot.
    if let Some(spec) = cli.check_openapi {
        let spec = spec.unwrap_or_else(picasu::openapi_parity::default_spec_path);
        std::process::exit(picasu::openapi_parity::run(&spec));
    }

    if cli.dump_openapi {
        print!("{}", picasu::openapi_public::public_json());
        return;
    }

    if cli.trivial_passwords {
        // SAFETY: `main` is still single-threaded here — no worker or
        // server thread has been spawned yet — so mutating the process
        // environment is sound. The flag wins over a preset env var: it is
        // the most explicit, closest-to-the-invocation setting.
        unsafe {
            std::env::set_var("PICASU_TRIVIAL_PASSWORDS", "true");
        }
    }

    if cli.bootstrap_admin {
        // SAFETY: see the `--trivial-passwords` bridge above — still
        // single-threaded, and the flag is the most explicit setting.
        unsafe {
            std::env::set_var("PICASU_BOOTSTRAP_ADMIN", "true");
        }
    }

    picasu::run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("picasu").chain(args.iter().copied()))
            .expect("test args must parse")
    }

    #[test]
    fn no_flags_boots_server_mode() {
        let cli = parse(&[]);
        assert!(!cli.dump_openapi);
        assert!(!cli.trivial_passwords);
        assert!(!cli.bootstrap_admin);
        assert!(cli.check_openapi.is_none());
    }

    #[test]
    fn dump_and_trivial_flags_parse() {
        let cli = parse(&["--dump-openapi", "--trivial-passwords"]);
        assert!(cli.dump_openapi);
        assert!(cli.trivial_passwords);
        assert!(cli.check_openapi.is_none());
    }

    #[test]
    fn bootstrap_admin_flag_parses() {
        let cli = parse(&["--bootstrap-admin"]);
        assert!(cli.bootstrap_admin);
        assert!(!cli.dump_openapi);
        assert!(!cli.trivial_passwords);
    }

    #[test]
    fn check_openapi_value_is_optional() {
        assert_eq!(parse(&["--check-openapi"]).check_openapi, Some(None));
        let cli = parse(&["--check-openapi", "spec.json"]);
        assert_eq!(cli.check_openapi, Some(Some(PathBuf::from("spec.json"))));
    }

    #[test]
    fn help_lists_every_flag() {
        let err = Cli::try_parse_from(["picasu", "--help"]).expect_err("--help exits via error");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
        let text = err.to_string();
        for flag in [
            "--dump-openapi",
            "--check-openapi",
            "--trivial-passwords",
            "--bootstrap-admin",
        ] {
            assert!(text.contains(flag), "help lists {flag}");
        }
    }

    #[test]
    fn unknown_flags_fail_fast() {
        let err = Cli::try_parse_from(["picasu", "--bogus"]).expect_err("unknown flag errors");
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }
}
