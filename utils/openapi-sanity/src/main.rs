//! `openapi-sanity check` — the source/spec half of `just openapi-check`.
//!
//! The gate answers one question: does the committed `OpenAPI` document describe
//! the source that claims to produce it? The build script already warns about
//! handlers without an annotation, and the backend contract tests compare the
//! spec against Rocket's mounted route table at runtime; what is left between
//! them is a comparison of the source itself, which needs no compiled artifact
//! and can therefore run before every spec regeneration.
//!
//! Everything here is I/O, formatting and exit codes. The rules live in
//! [`openapi_sanity::check_contract`], so the build script, the tests and this
//! binary cannot disagree about what counts as drift.
//!
//! # Exit codes
//!
//! | Code | Meaning                                                  |
//! | ---- | -------------------------------------------------------- |
//! | 0    | Nothing to report                                        |
//! | 1    | At least one contract finding, one per line on stderr    |
//! | 2    | The inputs could not be read: a missing file, a spec that is not JSON, a usage error |
//!
//! A failing gate prints one finding per line as `file:line: message`, and the
//! lines are sorted, so two runs over the same tree produce byte-identical
//! output. Nothing here reads the clock, the network or the environment beyond
//! the working directory used to shorten paths in diagnostics.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use openapi_sanity::{
    SCANNED_MODULES, SourceUnit, check_contract, referenced_handler_files, spec_operations,
};

const USAGE: &str = "\
openapi-sanity — source/spec contract checks for the picasu OpenAPI artifact

Usage:
    openapi-sanity check [options]
    openapi-sanity help

Options:
    --router-root <dir>     Router source root (default: backend/src/router)
    --spec <file>           Committed OpenAPI document (default: backend/openapi.json)
    --module <group>=<path> Router module whose routes![] block is part of the
                            contract, as a path relative to <router-root>.
                            Repeatable; replaces the built-in module list.
    --exclude-prefix <path> Operation path prefix that is deliberately outside
                            the public contract. Repeatable.

Exit codes: 0 nothing to report, 1 contract findings, 2 unusable input.";

/// Where the router sources live, relative to the repository root.
const DEFAULT_ROUTER_ROOT: &str = "backend/src/router";

/// The committed public document, relative to the repository root.
const DEFAULT_SPEC: &str = "backend/openapi.json";

/// Exit code for an input the gate could not act on.
const EXIT_UNUSABLE: u8 = 2;

/// One loaded source file, kept alive next to the borrowed [`SourceUnit`]s built
/// from it.
struct SourceFile {
    label: String,
    relative_path: String,
    source: String,
}

/// The paths and module list one run of the gate works on.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    router_root: PathBuf,
    spec: PathBuf,
    modules: Vec<(String, String)>,
    excluded_prefixes: Vec<String>,
}

/// What the arguments asked for.
#[derive(Debug, PartialEq, Eq)]
enum Invocation {
    Help,
    Check(Box<Options>),
}

/// What a completed run found.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// No finding; the number of compared operations, for the success line.
    Clean(usize),
    /// One finding per reported contract violation.
    Findings(Vec<String>),
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse(&args) {
        Ok(Invocation::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Invocation::Check(options)) => match run(&options) {
            Ok(outcome) => match outcome {
                Outcome::Clean(operations) => {
                    println!("openapi-sanity: no findings; {operations} spec operations checked");
                    ExitCode::SUCCESS
                }
                Outcome::Findings(findings) => {
                    for finding in &findings {
                        eprintln!("{finding}");
                    }
                    eprintln!("openapi-sanity: {} contract findings", findings.len());
                    ExitCode::from(1)
                }
            },
            Err(error) => fail(error),
        },
        Err(error) => fail(format!("{error}\n\n{USAGE}")),
    }
}

/// Report an input the gate could not act on, as opposed to a contract finding.
fn fail(error: impl std::fmt::Display) -> ExitCode {
    eprintln!("openapi-sanity: {error}");
    ExitCode::from(EXIT_UNUSABLE)
}

/// Read the arguments. Non-UTF-8 arguments cannot name a path the gate is meant
/// to open, so they are reported rather than silently dropped.
fn parse(args: &[OsString]) -> Result<Invocation, String> {
    let mut options = Options {
        router_root: PathBuf::from(DEFAULT_ROUTER_ROOT),
        spec: PathBuf::from(DEFAULT_SPEC),
        modules: Vec::new(),
        excluded_prefixes: Vec::new(),
    };
    let mut args = args.iter();

    let command = match args.next() {
        None => None,
        Some(arg) => Some(text(arg, "command")?),
    };
    match command.as_deref() {
        None | Some("check") => {}
        Some("help" | "--help" | "-h") => return Ok(Invocation::Help),
        Some(other) => return Err(format!("unknown command `{other}`")),
    }

    while let Some(arg) = args.next() {
        match text(arg, "option")?.as_str() {
            "--router-root" => {
                options.router_root = PathBuf::from(value(&mut args, "--router-root")?);
            }
            "--spec" => options.spec = PathBuf::from(value(&mut args, "--spec")?),
            "--module" => options
                .modules
                .push(module(&value(&mut args, "--module")?)?),
            "--exclude-prefix" => {
                options
                    .excluded_prefixes
                    .push(value(&mut args, "--exclude-prefix")?);
            }
            "--help" | "-h" => return Ok(Invocation::Help),
            other => return Err(format!("unknown option `{other}`")),
        }
    }

    Ok(Invocation::Check(Box::new(options)))
}

/// The value of an option that takes one.
fn value<'a>(
    args: &mut impl Iterator<Item = &'a OsString>,
    option: &str,
) -> Result<String, String> {
    let value = args
        .next()
        .ok_or_else(|| format!("option `{option}` needs a value"))?;
    text(value, option)
}

/// A `<group>=<path>` module argument.
fn module(argument: &str) -> Result<(String, String), String> {
    argument
        .split_once('=')
        .map(|(group, path)| (group.to_string(), path.to_string()))
        .ok_or_else(|| format!("`--module {argument}` must be written as <group>=<path>"))
}

/// An argument as text, or a diagnostic naming the option that carried it.
fn text(argument: &OsString, option: &str) -> Result<String, String> {
    argument
        .to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("{option} is not valid UTF-8"))
}

/// Load the router sources, read the document and run the checks.
fn run(options: &Options) -> Result<Outcome, String> {
    let modules = if options.modules.is_empty() {
        SCANNED_MODULES
            .iter()
            .map(|(group, path)| ((*group).to_string(), (*path).to_string()))
            .collect()
    } else {
        options.modules.clone()
    };

    let mut files = Vec::new();
    for (_, relative_path) in &modules {
        files.push(read(&options.router_root, relative_path)?);
    }
    for relative_path in handler_files(&files, &modules) {
        files.push(read(&options.router_root, &relative_path)?);
    }

    let units: Vec<SourceUnit<'_>> = files.iter().map(unit).collect();
    let document = read_spec(&options.spec)?;
    let spec = spec_operations(&document);
    let excluded: Vec<&str> = options
        .excluded_prefixes
        .iter()
        .map(String::as_str)
        .collect();
    let findings: Vec<String> = check_contract(&units, &label_for(&options.spec), &spec, &excluded)
        .iter()
        .map(ToString::to_string)
        .collect();

    Ok(if findings.is_empty() {
        let compared = spec
            .iter()
            .filter(|operation| {
                !excluded
                    .iter()
                    .any(|prefix| operation.path.starts_with(prefix))
            })
            .count();
        Outcome::Clean(compared)
    } else {
        Outcome::Findings(findings)
    })
}

/// The router-relative files defining the handlers the route tables name, minus
/// the ones already loaded as route tables.
fn handler_files(files: &[SourceFile], modules: &[(String, String)]) -> Vec<String> {
    let units: Vec<SourceUnit<'_>> = files.iter().map(unit).collect();
    referenced_handler_files(&units)
        .into_iter()
        .filter(|path| !modules.iter().any(|(_, listed)| listed == path))
        .collect()
}

/// A loaded file as a unit, with the group and module derived from its path.
fn unit(file: &SourceFile) -> SourceUnit<'_> {
    SourceUnit::for_relative_path(&file.label, &file.relative_path, &file.source)
}

/// Read one router file.
///
/// A module of the scanned list whose file is missing fails the run: the list is
/// the contract, and scanning one module fewer would shrink it without saying so.
fn read(router_root: &Path, relative_path: &str) -> Result<SourceFile, String> {
    let path = router_root.join(relative_path);
    let source = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read router module {}: {error}", path.display()))?;
    Ok(SourceFile {
        label: label_for(&path),
        relative_path: relative_path.to_string(),
        source,
    })
}

/// Read and parse the committed document.
///
/// A document without a `paths` object is rejected here rather than left to
/// [`spec_operations`], which treats one as a programming error and panics. The
/// difference matters to a gate: a malformed artifact is unusable input, which
/// has an exit code of its own.
fn read_spec(path: &Path) -> Result<serde_json::Value, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read spec {}: {error}", path.display()))?;
    let document: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not valid JSON: {error}", path.display()))?;
    if document
        .get("paths")
        .and_then(serde_json::Value::as_object)
        .is_none()
    {
        return Err(format!("{} has no `paths` object", path.display()));
    }
    Ok(document)
}

/// A diagnostic label for a file, shortened against the working directory so the
/// report is the same wherever the gate runs from.
fn label_for(path: &Path) -> String {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(cwd).ok().map(Path::to_path_buf))
        .unwrap_or_else(|| path.to_path_buf())
        .display()
        .to_string()
}
