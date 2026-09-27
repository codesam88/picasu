//! The `openapi-sanity check` binary: what it reads, what it prints, and what it
//! exits with.
//!
//! The rules are covered in `contract.rs`; what is left to assert here is the
//! half the library does not own — that the gate reads the files it is told to
//! read, prints one finding per line, and fails the process when it finds one.
//! The fixture paths are relative on purpose: the gate shortens labels against
//! the working directory, and a test that depended on where the checkout lives
//! could not state an expected diagnostic.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use openapi_sanity::spec_operations;

/// The prefix the public artifact omits on purpose, as the gate is run in the
/// repository.
const TEST_PREFIX: &str = "/get/test/";

/// The one scanned module of a fixture tree, as `--module` takes it.
const FIXTURE_MODULE: &str = "get=get/mod.rs";

#[test]
fn a_conforming_tree_exits_zero_with_nothing_on_stderr() {
    let output = run("clean", &[]);

    assert_clean(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "openapi-sanity: no findings; 5 spec operations checked",
        "the success line states what was compared, so a run that compared nothing \
         is distinguishable from one that found nothing"
    );
}

#[test]
fn a_drifted_tree_exits_nonzero_with_one_diagnostic_per_line() {
    let output = run("drift", &[]);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let mut lines: Vec<&str> = stderr.lines().collect();
    let summary = lines.pop().expect("a summary line after the findings");

    assert_eq!(output.status.code(), Some(1), "stderr was:\n{stderr}");
    assert_eq!(
        lines,
        [
            "tests/fixtures/drift/get/data.rs:8: data::get_data: the route serves \
             /get/get-data but its #[utoipa::path] declares /get/get-data-RENAMED",
            "tests/fixtures/drift/get/data.rs:14: data::get_rows: the route declares GET \
             but its #[utoipa::path] declares POST",
            "tests/fixtures/drift/get/data.rs:20: data::path_completion: GET \
             /get/path-completion is declared in source but absent from the spec",
            "tests/fixtures/drift/get/data.rs:24: data::get_metadata: registered in \
             routes![] but the function carries no #[utoipa::path] annotation",
            "tests/fixtures/drift/get/mod.rs:10: page::login: registered in routes![] \
             more than once (first at tests/fixtures/drift/get/mod.rs:9)",
            "tests/fixtures/drift/openapi.json: GET /get/get-albums is in the spec but \
             no scanned route declares it",
            "tests/fixtures/drift/openapi.json: duplicate operationId `get_data` claimed \
             by GET /get/get-albums, GET /get/get-data-RENAMED",
        ],
        "one finding per line, in the order the gate sorted them"
    );
    assert_eq!(summary, "openapi-sanity: 7 contract findings");
    assert!(
        output.stdout.is_empty(),
        "findings belong on stderr so the summary stays the only thing on stdout"
    );
}

#[test]
fn two_runs_over_one_tree_print_the_same_report() {
    let first = run("drift", &[]);
    let second = run("drift", &[]);

    assert_eq!(first.stdout, second.stdout);
    assert_eq!(first.stderr, second.stderr);
    assert_eq!(first.status.code(), second.status.code());
}

#[test]
fn the_module_list_can_be_replaced_from_the_command_line() {
    // Naming the one module of a fixture tree is what lets the gate run against
    // something other than the repository, which is the only way the drift
    // fixtures reach it.
    let output = run("drift", &[]);

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("7 contract findings"));
}

#[test]
fn a_module_that_cannot_be_read_fails_the_run() {
    // A scanned module whose file is gone would shrink the contract silently.
    let output = run("clean", &["--module", "get=get/absent.rs"]);

    assert_unusable(
        &output,
        "cannot read router module tests/fixtures/clean/get/absent.rs",
    );
}

#[test]
fn a_spec_that_cannot_be_read_fails_the_run() {
    let output = run("clean", &["--spec", "tests/fixtures/clean/absent.json"]);

    assert_unusable(&output, "cannot read spec tests/fixtures/clean/absent.json");
}

#[test]
fn a_spec_that_is_not_json_fails_the_run() {
    let output = run("clean", &["--spec", "tests/fixtures/clean/get/mod.rs"]);

    assert_unusable(&output, "is not valid JSON");
}

#[test]
fn a_spec_without_paths_fails_the_run() {
    let output = run("clean", &["--spec", "tests/fixtures/clean/no_paths.json"]);

    assert_unusable(&output, "no `paths` object");
}

#[test]
fn an_unknown_option_fails_with_the_usage() {
    let output = gate(&["check", "--nonsense"]);

    assert_unusable(&output, "unknown option `--nonsense`");
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
}

#[test]
fn an_unknown_command_fails_with_the_usage() {
    let output = gate(&["explain"]);

    assert_unusable(&output, "unknown command `explain`");
}

#[test]
fn an_option_without_its_value_fails_with_the_usage() {
    let output = gate(&["check", "--spec"]);

    assert_unusable(&output, "option `--spec` needs a value");
}

#[test]
fn a_module_argument_without_a_group_fails_with_the_usage() {
    let output = gate(&["check", "--module", "get/mod.rs"]);

    assert_unusable(
        &output,
        "`--module get/mod.rs` must be written as <group>=<path>",
    );
}

#[test]
fn help_exits_zero_and_documents_the_options() {
    for argument in ["help", "--help", "-h"] {
        let output = gate(&[argument]);
        let stdout = String::from_utf8_lossy(&output.stdout);

        assert_eq!(output.status.code(), Some(0), "`{argument}` should succeed");
        assert!(
            stdout.contains("Usage:"),
            "`{argument}` should print the usage"
        );
        for option in ["--router-root", "--spec", "--module", "--exclude-prefix"] {
            assert!(
                stdout.contains(option),
                "`{argument}` should document {option}"
            );
        }
    }
}

#[test]
fn the_repository_is_currently_clean() {
    // The gate runs in `just check` and in CI, so the tree it reads has to pass
    // today. Without this, a rule that stopped comparing anything would leave
    // the fixture tests failing over a *fixture* while the real tree went
    // unexamined.
    let repository = repository_root();
    let output = Command::new(env!("CARGO_BIN_EXE_openapi-sanity"))
        .current_dir(&repository)
        .args(["check", "--exclude-prefix", TEST_PREFIX])
        .output()
        .expect("the gate binary runs");

    assert_clean(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!(
            "openapi-sanity: no findings; {} spec operations checked",
            documented_operations(&repository)
        )
    );
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// The gate over a fixture tree, run from the crate directory so the labels in
/// its report are the `tests/fixtures/...` paths the assertions above state.
fn run(tree: &str, extra: &[&str]) -> Output {
    let mut args = vec![
        "check".to_string(),
        "--router-root".to_string(),
        format!("tests/fixtures/{tree}"),
        "--spec".to_string(),
        format!("tests/fixtures/{tree}/openapi.json"),
        "--module".to_string(),
        FIXTURE_MODULE.to_string(),
        "--exclude-prefix".to_string(),
        TEST_PREFIX.to_string(),
    ];
    args.extend(extra.iter().map(ToString::to_string));

    gate(&args.iter().map(String::as_str).collect::<Vec<_>>())
}

/// Run the gate with the crate directory as the working directory.
fn gate(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_openapi-sanity"))
        .current_dir(crate_root())
        .args(args)
        .output()
        .expect("the gate binary runs")
}

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repository_root() -> PathBuf {
    crate_root()
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits in <repo>/utils/openapi-sanity")
        .to_path_buf()
}

/// The number of operations the repository document declares outside the
/// test-only prefix, which is what the success line counts.
fn documented_operations(repository: &Path) -> usize {
    let document = std::fs::read_to_string(repository.join("backend").join("openapi.json"))
        .expect("the committed document is readable");
    let document: serde_json::Value =
        serde_json::from_str(&document).expect("the committed document is valid JSON");

    spec_operations(&document)
        .iter()
        .filter(|operation| !operation.path.starts_with(TEST_PREFIX))
        .count()
}

fn assert_clean(output: &Output) {
    assert!(
        output.stderr.is_empty(),
        "a clean gate says nothing on stderr, got:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.status.code(), Some(0));
}

fn assert_unusable(output: &Output, expected: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(2), "stderr was:\n{stderr}");
    assert!(
        stderr.contains(expected),
        "expected `{expected}` in:\n{stderr}"
    );
}
