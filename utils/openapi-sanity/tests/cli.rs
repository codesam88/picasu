//! The CLI contract the gate recipe depends on.
//!
//! The rules are checked in `openapi_annotations.rs`; what is checked here is the
//! reporting and exit-code contract around them, including the coverage floor —
//! the run has to fail when the scan saw too little to stand behind a "clean",
//! because a narrowed walk and a clean tree produce the same report.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The binary this test drives, as built for this test run.
const BINARY: &str = env!("CARGO_BIN_EXE_openapi-sanity");

/// The backend's router tree, resolved from this crate's manifest directory
/// rather than from the working directory a test happens to run in.
fn router_tree() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../backend/src/router")
}

/// The fixture tree, which has findings in it.
fn fixture_tree() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/openapi_annotations")
}

/// Run the CLI over `source_root` with `args` after them, from the workspace root
/// so that the source root is passed the way the recipe passes it.
fn run(source_root: &Path, args: &[&str]) -> Output {
    let root = source_root
        .canonicalize()
        .unwrap_or_else(|error| panic!("{} must exist: {error}", source_root.display()));
    Command::new(BINARY)
        .current_dir(workspace_root())
        .arg("--source-root")
        .arg(root)
        .args(args)
        .output()
        .expect("the CLI must run")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate lives two directories below the workspace root")
        .to_owned()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// How many annotated handlers the real tree carries, read through the library
/// rather than pinned here: the calibration is pinned once, in
/// `openapi_annotations.rs`. This test is about the CLI reporting the number, not
/// about the number.
fn annotated_handlers_in_the_router_tree() -> usize {
    openapi_sanity::scan_source_root(
        &router_tree(),
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../backend/src/error.rs"),
    )
    .expect("the router tree must be readable")
    .annotated_handlers()
}

#[test]
fn a_clean_tree_above_the_floor_passes() {
    let handlers = annotated_handlers_in_the_router_tree();

    let output = run(
        &router_tree(),
        &["--expect-at-least", &handlers.to_string()],
    );

    assert!(
        output.status.success(),
        "a tree at the floor is not below it: {}",
        stderr(&output)
    );
    assert_eq!(
        stdout(&output).trim(),
        format!(
            "openapi-sanity: no findings in {handlers} annotated handler(s) under \
             backend/src/router"
        ),
        "the summary names the observed count and the source root relative to the \
         workspace, not the absolute path it was given"
    );
}

/// The blindness check: a floor the tree cannot meet fails the run instead of
/// reporting a tree the scan never fully read.
#[test]
fn a_tree_below_the_floor_fails() {
    let handlers = annotated_handlers_in_the_router_tree();
    let floor = handlers + 1;

    let output = run(&router_tree(), &["--expect-at-least", &floor.to_string()]);

    assert!(
        !output.status.success(),
        "a scan below the floor must not exit 0: {}",
        stdout(&output)
    );
    let reported = stderr(&output);
    assert!(
        reported.contains(&format!(
            "the scan found {handlers} annotated handler(s), fewer than the expected \
             minimum of {floor}"
        )),
        "the shortfall names both counts: {reported}"
    );
    assert!(
        reported.contains("a narrowed file walk is the likely cause"),
        "the message must point at the walk rather than at the source: {reported}"
    );
}

/// Coverage outranks findings: a short scan's findings are not the news, because
/// they only describe the part of the tree that was reached.
#[test]
fn a_short_scan_reports_coverage_rather_than_findings() {
    let output = run(&fixture_tree(), &["--expect-at-least", "61"]);

    assert!(!output.status.success());
    let reported = stderr(&output);
    assert!(
        reported.contains("fewer than the expected minimum of 61"),
        "{reported}"
    );
    assert!(
        !reported.contains("no_responses"),
        "findings from a short scan must not be reported as the failure: {reported}"
    );
}

/// Without a floor the tool reports what it saw and nothing more — the floor is
/// opt-in, and it is the recipe that supplies it.
#[test]
fn findings_still_report_the_observed_count() {
    let output = run(&fixture_tree(), &[]);

    assert!(!output.status.success());
    let reported = stderr(&output);
    assert!(
        reported.contains("no_responses"),
        "the findings themselves still reach the console: {reported}"
    );
    assert!(
        reported.contains(
            "finding(s) across 60 annotated handler(s) under \
             utils/openapi-sanity/tests/fixtures/openapi_annotations",
        ),
        "the summary keeps the observed count: {reported}"
    );
}

/// A flag nobody can act on is a failure, not a finding: the two are different
/// problems and get different exit codes.
#[test]
fn an_unusable_flag_exits_two() {
    for args in [
        vec!["--expect-at-least", "many"],
        vec!["--expect-at-least"],
        vec!["--nonsense"],
    ] {
        let output = run(&router_tree(), &args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?} must exit 2, not a finding code: {}",
            stderr(&output)
        );
    }
}
