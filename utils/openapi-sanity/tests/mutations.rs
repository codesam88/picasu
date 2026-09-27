//! What the auth rules do when the thing they watch is broken.
//!
//! The fixture tests pin a rule from a tree that is already drifted, which shows
//! a rule reports but not that it is the *only* thing that reports it. These
//! tests start from a conforming tree, break exactly one thing, and require the
//! rule to appear — then restore the tree and require silence, so a rule that
//! fires for any reason at all fails here.
//!
//! The mutations run over a copy written to `CARGO_TARGET_TMPDIR`: the checked-in
//! fixture has to stay conforming, or the other tests stop having a baseline.

use std::path::{Path, PathBuf};

use openapi_sanity::{AUTH_POLICY, AuthRule};

mod support;

use support::{CLEAN_POLICY, Fixture};

/// A router tree and document that agree, including about authentication.
const CLEAN: &str = "clean";

/// The guard a mutation drops, as it appears in the fixture.
const PROTECTED_GUARD: &str = "_auth: &GuardAuth";

#[test]
fn removing_a_guard_from_a_protected_handler_fails() {
    let tree = materialise("removed-guard");
    assert_eq!(
        tree.auth_findings(CLEAN_POLICY),
        Vec::<String>::new(),
        "the unmutated copy of the fixture is the baseline these mutations are \
         measured against"
    );

    let data = tree.source("get/data.rs");
    assert!(data.contains(PROTECTED_GUARD), "the fixture lost its guard");
    tree.replace(
        "get/data.rs",
        &data.replace(PROTECTED_GUARD, "_unused: &str"),
    );

    assert_eq!(
        tree.auth_findings(CLEAN_POLICY),
        vec![format!(
            "{}/get/data.rs:15: data::get_rows: the auth policy requires GuardAuth but \
             the handler declares no request guard",
            tree.label()
        )],
        "a protected handler that stopped guarding itself is the drift this rule \
         exists for"
    );
}

#[test]
fn dropping_a_guard_result_fails() {
    let tree = materialise("dropped-guard-result");
    assert_eq!(tree.auth_findings(CLEAN_POLICY), Vec::<String>::new());

    let data = tree.source("get/data.rs");
    assert!(
        data.contains("let _ = guard_timestamp?;"),
        "the fixture lost its propagated guard"
    );
    tree.replace(
        "get/data.rs",
        &data.replace("let _ = guard_timestamp?;", "let _ = guard_timestamp;"),
    );

    assert_eq!(
        tree.auth_findings(CLEAN_POLICY),
        vec![format!(
            "{}/get/data.rs:7: get_data: the deferred guard GuardTimestamp is bound to \
             `guard_timestamp` and never enforced — `let _ = guard_timestamp;` drops \
             its failure; propagate it with `?`, return it, or match on it",
            tree.label()
        )],
        "a deferred guard the handler throws away serves an unauthorized caller, \
         which is the bug this codebase has already had"
    );
}

#[test]
fn dropping_a_public_operation_from_the_policy_fails() {
    // The other direction of the same table: a new operation nobody classified is
    // an omission, not a pass.
    let tree = materialise("unlisted-operation");
    assert_eq!(tree.auth_findings(CLEAN_POLICY), Vec::<String>::new());

    let without_login: Vec<AuthRule> = CLEAN_POLICY
        .iter()
        .filter(|rule| rule.operation_id != "login")
        .copied()
        .collect();
    let findings = tree.auth_findings(&without_login);

    assert_eq!(
        findings,
        vec![format!(
            "{}/openapi.json: GET /login (operationId `login`) is in no auth policy \
             entry — add one naming its guards, or one marking it a public exception",
            tree.label()
        )],
        "an operation in no policy entry has an unstated authentication requirement, \
         which is not the same as an open one"
    );
}

#[test]
fn the_repository_policy_is_not_a_fixture_policy() {
    // A guard against the obvious accident: a test that made the mutations pass by
    // checking them against a table with no entries in it. `AUTH_POLICY` has to
    // describe the real API, and the real API's operations are not the fixture's.
    let repository = repository_root();
    let router = Fixture::in_directory(&repository.join("backend").join("src").join("router"));

    assert_eq!(
        router.auth_findings_against(
            &repository.join("backend").join("openapi.json"),
            &["/get/test/"],
            AUTH_POLICY
        ),
        Vec::<String>::new()
    );
    assert!(
        CLEAN_POLICY.len() < AUTH_POLICY.len(),
        "the fixture policy covers only the fixture's operations"
    );
}

// ── The mutated tree ──────────────────────────────────────────────────────────

/// A writable copy of a fixture tree, plus the label its findings carry.
struct Tree {
    label: String,
    root: PathBuf,
}

impl Tree {
    fn label(&self) -> &str {
        &self.label
    }

    /// The current contents of one of its files.
    fn source(&self, relative: &str) -> String {
        support::read(&self.root.join(relative))
    }

    fn replace(&self, relative: &str, source: &str) {
        std::fs::write(self.root.join(relative), source)
            .unwrap_or_else(|error| panic!("{relative} is rewritten: {error}"));
    }

    /// The auth gate's report for the tree as it now stands.
    fn auth_findings(&self, policy: &[AuthRule]) -> Vec<String> {
        Fixture::in_directory(&self.root).auth_findings_against(
            &self.root.join("openapi.json"),
            &[],
            policy,
        )
    }
}

/// A copy of a fixture tree in a directory of its own, so a mutation cannot reach
/// the checked-in fixture.
fn materialise(name: &str) -> Tree {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("the mutation directory is created");
    let tree = Fixture::load(CLEAN).materialise(&root);

    // The document is copied too, and the gate labels it from the same directory,
    // so a finding names a file inside the copy rather than the fixture.
    let _ = tree;
    Tree {
        label: root.display().to_string(),
        root,
    }
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits in <repo>/utils/openapi-sanity")
        .to_path_buf()
}
