fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    apply_cli_flags(&args);

    // A sibling of `--dump-openapi`, and the only reason the committed spec is
    // read at all: the route-set gate runs when asked for, never at boot.
    if let Some(index) = args.iter().position(|arg| arg == "--check-openapi") {
        let spec = args
            .get(index + 1)
            .filter(|next| !next.starts_with('-'))
            .map_or_else(picasu::openapi_parity::default_spec_path, Into::into);
        std::process::exit(picasu::openapi_parity::run(&spec));
    }

    if args.iter().any(|arg| arg == "--dump-openapi") {
        print!("{}", picasu::openapi_public::public_json());
        return;
    }

    picasu::run();
}

/// Flags that map onto `PICASU_*` env overrides, applied before the server
/// boots and reads them. The flag wins over a preset env var: it is the
/// most explicit, closest-to-the-invocation setting. New boolean flags
/// follow the `--trivial-passwords` entry below.
fn apply_cli_flags(args: &[String]) {
    if cli_flag_present(args, "--trivial-passwords") {
        // SAFETY: `main` is still single-threaded here — no worker or
        // server thread has been spawned yet — so mutating the process
        // environment is sound.
        unsafe {
            std::env::set_var("PICASU_TRIVIAL_PASSWORDS", "true");
        }
    }
}

/// Pure detection half of [`apply_cli_flags`], kept separate so the flag
/// parsing is unit-testable without touching the process environment.
fn cli_flag_present(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(flags: &[&str]) -> Vec<String> {
        flags.iter().map(|flag| (*flag).to_string()).collect()
    }

    #[test]
    fn trivial_passwords_flag_is_detected() {
        assert!(cli_flag_present(
            &args(&["--trivial-passwords"]),
            "--trivial-passwords"
        ));
        assert!(!cli_flag_present(
            &args(&["--dump-openapi"]),
            "--trivial-passwords"
        ));
        assert!(!cli_flag_present(&args(&[]), "--trivial-passwords"));
        // Prefix lookalikes do not count.
        assert!(!cli_flag_present(
            &args(&["--trivial-passwords=x"]),
            "--trivial-passwords"
        ));
    }
}
