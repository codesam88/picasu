//! What the rules do when the thing they watch is broken.
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

/// The subject a mutation adds, chosen because the taxonomy deliberately has no
/// `metadata` — its operations belong to `assets`.
const UNKNOWN_TAG: &str = "metadata";

/// The data-API operation the tag mutations act on.
const DATA_OPERATION: &str = "/get/get-data";

/// The subject that operation carries in the fixture.
const TIMELINE_TAG: &str = "timeline";

/// The subject reserved for the SPA page routes.
const PAGE_TAG: &str = "pages";

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

// ── The tag taxonomy ──────────────────────────────────────────────────────────

/// A copy of the conforming tree with one tag changed in its document.
///
/// Every tag rule is measured against the same silent baseline, and each mutation
/// changes exactly one tag, so a finding a neighbouring rule also makes shows up as
/// an extra line in the exact expectation below.
fn mutated_document(name: &str, change: impl FnOnce(&mut serde_json::Value)) -> Tree {
    let tree = materialise(name);
    assert_eq!(
        tree.tag_findings(),
        Vec::<String>::new(),
        "the unmutated copy of the fixture is the baseline these mutations are \
         measured against"
    );
    tree.change_document(change);
    tree
}

#[test]
fn dropping_a_tag_from_an_operation_fails() {
    // What a `#[utoipa::path]` edit looks like after a regeneration: the
    // annotation kept its path, its verb and its responses, and lost its subject.
    let tree = mutated_document("untagged-operation", |document| {
        let operation = document["paths"][DATA_OPERATION]["get"]
            .as_object_mut()
            .expect("a path item's operation is an object");
        operation.remove("tags");
    });

    assert_eq!(
        tree.tag_findings(),
        vec![format!(
            "{}/openapi.json: GET {DATA_OPERATION}: declares no tags",
            tree.label()
        )],
        "an operation no subject can be read off is the drift the reference groups \
         by nothing"
    );
    tree.restore(DATA_OPERATION, TIMELINE_TAG);
}

#[test]
fn tagging_an_operation_outside_the_taxonomy_fails() {
    let tree = mutated_document("unknown-tag", |document| {
        document["paths"][DATA_OPERATION]["get"]["tags"] = serde_json::json!([UNKNOWN_TAG]);
    });

    assert_eq!(
        tree.tag_findings(),
        vec![format!(
            "{}/openapi.json: GET {DATA_OPERATION}: unknown tag `{UNKNOWN_TAG}`",
            tree.label()
        )],
        "a subject nobody reviewed is a reference group of one, and it groups under \
         a name the documentation does not explain"
    );
    tree.restore(DATA_OPERATION, TIMELINE_TAG);
}

#[test]
fn tagging_a_data_operation_pages_fails() {
    // The mistake the reserved tag exists to catch: a data operation wearing the
    // SPA page tag, which puts it in the same reference section as the shell.
    let tree = mutated_document("data-operation-tagged-pages", |document| {
        document["paths"][DATA_OPERATION]["get"]["tags"] = serde_json::json!([PAGE_TAG]);
    });

    assert_eq!(
        tree.tag_findings(),
        vec![format!(
            "{}/openapi.json: GET {DATA_OPERATION}: data-API path carries the `pages` tag",
            tree.label()
        )]
    );
    tree.restore(DATA_OPERATION, TIMELINE_TAG);
}

#[test]
fn a_page_operation_that_loses_its_pages_tag_fails() {
    let tree = mutated_document("page-without-pages-tag", |document| {
        document["paths"]["/login"]["get"]["tags"] = serde_json::json!(["auth"]);
    });

    assert_eq!(
        tree.tag_findings(),
        vec![format!(
            "{}/openapi.json: GET /login: SPA page path must carry `pages`",
            tree.label()
        )],
        "the other direction of the reserved tag: a page route grouped under a \
         subject, which is how the shell ends up inside the data sections"
    );
    tree.restore("/login", PAGE_TAG);
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

    /// The tag gate's report for the tree's document, labelled as the tree's.
    ///
    /// Document-shaped, so the router sources are not read: the taxonomy is about
    /// what the generated reference groups by, and a mutation edits the document a
    /// regeneration would have written.
    fn tag_findings(&self) -> Vec<String> {
        support::tag_findings_against(&self.root.join("openapi.json"), &[])
    }

    /// Put an operation's subject back and require the tree to be silent again, so
    /// a rule that fires for any reason at all fails here rather than passing on
    /// the strength of the mutation's own finding.
    fn restore(&self, path: &str, tag: &str) {
        self.change_document(|document| {
            document["paths"][path]["get"]["tags"] = serde_json::json!([tag]);
        });
        assert_eq!(
            self.tag_findings(),
            Vec::<String>::new(),
            "{path} is conforming again, so the finding above has to be the mutation's"
        );
    }

    /// Edit the tree's document in place, so a mutation can be applied and
    /// reverted without string surgery on JSON.
    fn change_document(&self, change: impl FnOnce(&mut serde_json::Value)) {
        let path = self.root.join("openapi.json");
        let mut document: serde_json::Value = serde_json::from_str(&support::read(&path))
            .expect("the fixture document is valid JSON");
        change(&mut document);
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&document).expect("the document serializes"),
        )
        .expect("the document is written");
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
