//! Console entry point: run the annotation checks over a source root and report
//! what they found.
//!
//! The exit code is the whole contract with the gate recipe. It follows the
//! contract gate's other tools — `0` clean, `1` findings or a scan too small to
//! stand behind, `2` an input that cannot be read, which is a different failure
//! from the source being wrong and is not a passing run either.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use openapi_sanity::{TreeReport, render, scan_source_root};

/// The tree the tool checks by default.
///
/// Relative to the workspace root, which is the working directory cargo runs
/// rustc in — the same reason `#[utoipauto]` is written `./backend/src/router`
/// rather than a manifest-relative path. The gate recipe passes an absolute path
/// instead, so that running it from a subdirectory checks the same tree.
const DEFAULT_SOURCE_ROOT: &str = "backend/src/router";

/// How the run was asked for.
struct Options {
    source_root: PathBuf,
    /// A floor on the annotated handlers the scan must see for its "clean" to
    /// mean anything. `None` when the run was not given one.
    expect_at_least: Option<usize>,
}

fn main() -> ExitCode {
    match run(std::env::args().skip(1)) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("openapi-sanity: {message}");
            ExitCode::from(2)
        }
    }
}

/// Run the check and report it on the console. The return value is the process
/// exit code, which is all the caller does with it.
fn run(args: impl Iterator<Item = String>) -> Result<ExitCode, String> {
    let options = options(args)?;

    let report = scan_source_root(&options.source_root).map_err(|error| error.to_string())?;
    Ok(exit_code(&report, &options))
}

/// The verdict for a completed scan.
fn exit_code(report: &TreeReport, options: &Options) -> ExitCode {
    let root = display_path(&options.source_root);
    let observed = report.annotated_handlers();

    // Before the findings, and overriding them: a scan below the floor has not
    // established that the tree is clean, so its findings are not the news.
    if let Some(expected) = options.expect_at_least
        && let Err(shortfall) = report.check_coverage(expected)
    {
        eprintln!("openapi-sanity: {shortfall} under {root}");
        eprintln!(
            "Check the file walk and the source root before trusting this run; the recipe \
             sets the floor, so a change to it is a change to what the gate covers."
        );
        return ExitCode::FAILURE;
    }

    if report.is_clean() {
        println!("openapi-sanity: no findings in {observed} annotated handler(s) under {root}");
        return ExitCode::SUCCESS;
    }

    eprintln!("{}", render(&report.findings));
    eprintln!(
        "openapi-sanity: {} finding(s) across {observed} annotated handler(s) under {root}",
        report.findings.len()
    );
    ExitCode::FAILURE
}

/// Render a source root relative to the workspace root, so the report reads the
/// same on every machine. Falls back to the path as given when the root is not
/// inside this workspace — a path outside it is not ours to shorten.
fn display_path(path: &Path) -> String {
    match workspace_root().and_then(|root| path.strip_prefix(root).ok()) {
        Some(relative) if relative.as_os_str().is_empty() => ".".to_owned(),
        Some(relative) => relative.to_string_lossy().replace('\\', "/"),
        None => path.to_string_lossy().replace('\\', "/"),
    }
}

/// The workspace this crate belongs to: the nearest ancestor of the crate's
/// manifest directory with a `[workspace]` of its own. Resolved from the compile
/// time manifest directory rather than from the working directory, which is
/// wherever cargo or just happened to be invoked.
fn workspace_root() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|directory| {
            std::fs::read_to_string(directory.join("Cargo.toml"))
                .is_ok_and(|manifest| manifest.contains("[workspace]"))
        })
        .map(Path::to_path_buf)
}

/// Read the flags: `--source-root <dir>` and `--expect-at-least <n>`, the latter
/// defaulting to no floor.
///
/// Written out rather than pulled from an argument-parsing crate: there are two
/// flags, and a dependency for them would be more surface than the flags.
fn options(args: impl Iterator<Item = String>) -> Result<Options, String> {
    let usage =
        "usage: openapi-sanity [--source-root <dir>] [--expect-at-least <annotated-handlers>]";
    let mut source_root = PathBuf::from(DEFAULT_SOURCE_ROOT);
    let mut expect_at_least = None;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        if let Some(value) = flag_value(&arg, "--source-root", &mut args)? {
            source_root = PathBuf::from(value);
        } else if let Some(value) = flag_value(&arg, "--expect-at-least", &mut args)? {
            expect_at_least = Some(
                value
                    .parse()
                    .map_err(|_| format!("--expect-at-least needs a number, got `{value}`"))?,
            );
        } else {
            return Err(format!("unknown argument `{arg}`; {usage}"));
        }
    }

    Ok(Options {
        source_root,
        expect_at_least,
    })
}

/// The value of `flag`, from the next argument or from an `=`, or `None` when
/// `arg` is not that flag at all.
fn flag_value(
    arg: &str,
    flag: &str,
    args: &mut impl Iterator<Item = String>,
) -> Result<Option<String>, String> {
    if arg == flag {
        return args
            .next()
            .map(Some)
            .ok_or_else(|| format!("{flag} needs a value"));
    }
    Ok(arg.strip_prefix(&format!("{flag}=")).map(ToOwned::to_owned))
}
