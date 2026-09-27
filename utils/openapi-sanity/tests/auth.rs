//! The auth policy gate: one test per way the source can disagree with it.
//!
//! The policy is a table with an entry per operation, so every rule here is
//! driven from a *conforming* baseline as well: the `unauthored/` tree carries one
//! instance of every failure mode and the report is asserted as a whole, which is
//! what makes a rule that stopped reporting — or started reporting twice — a test
//! failure rather than a quieter gate.

use openapi_sanity::{AUTH_POLICY, AuthRule, GuardClass};

mod support;

use support::{CLEAN_POLICY, Fixture, TEST_PREFIX};

/// A router tree and document carrying one instance of every auth failure mode.
const UNAUTHORED: &str = "unauthored";

/// A router tree and document that agree, including about authentication.
const CLEAN: &str = "clean";

/// The policy the `unauthored/` fixture is checked against, and the shape the
/// crate's own policy has: one entry per operation, guarded entries naming their
/// guards and public ones stating they are open.
const FIXTURE_POLICY: &[AuthRule] = &[
    AuthRule::guarded("get_data", &[GuardClass::Timestamp]),
    AuthRule::guarded("get_rows", &[GuardClass::AdminCookie]),
    AuthRule::guarded("get_scroll_bar", &[GuardClass::Timestamp]),
    AuthRule::guarded("get_tags", &[GuardClass::AdminCookie]),
    AuthRule::public("login"),
    AuthRule::public("trashed"),
    // Drift: an entry for an operation the document does not declare.
    AuthRule::public("renamed_operation"),
];

// ── A conforming tree ─────────────────────────────────────────────────────────

#[test]
fn a_router_whose_policy_matches_reports_nothing() {
    let fixture = Fixture::load(CLEAN);

    assert_eq!(
        fixture.auth_findings(&[TEST_PREFIX], CLEAN_POLICY),
        Vec::<String>::new(),
        "a tree whose guards, its document and its policy agree must not be reported"
    );
}

#[test]
fn the_clean_fixture_covers_both_guard_shapes() {
    // The clean tree is the baseline every other auth expectation is read
    // against, so it has to exercise a direct guard, a qualified path and a
    // deferred binding rather than only the easiest shape.
    let fixture = Fixture::load(CLEAN);
    let data = fixture.read("get/data.rs");

    assert!(
        data.contains("GuardResult<GuardTimestamp>") && data.contains("let _ = guard_timestamp?;"),
        "the clean fixture propagates a deferred guard"
    );
    assert!(
        data.contains("&GuardAuth"),
        "the clean fixture has a bare guard"
    );
    assert!(
        data.contains("auth::GuardAuth"),
        "the clean fixture names a guard by its path"
    );
}

// ── One test per failure mode ─────────────────────────────────────────────────

#[test]
fn reports_a_protected_operation_whose_handler_declares_no_guard() {
    let fixture = Fixture::load(UNAUTHORED);

    fixture.assert_auth_reports(&format!(
        "{}:15: data::get_rows: the auth policy requires GuardAuth but the handler \
         declares no request guard",
        fixture.label("get/data.rs")
    ));
}

#[test]
fn reports_a_guarded_operation_that_documents_no_unauthorized_response() {
    let fixture = Fixture::load(UNAUTHORED);

    fixture.assert_auth_reports(&format!(
        "{}: GET /get/get-tags is guarded but documents no 401 response — add \
         `(status = 401, response = Unauthorized)` to its #[utoipa::path]",
        fixture.label("openapi.json")
    ));
}

#[test]
fn reports_a_public_operation_that_documents_an_unexpected_unauthorized_response() {
    let fixture = Fixture::load(UNAUTHORED);

    fixture.assert_auth_reports(&format!(
        "{}: GET /trashed is a public operation but documents a 401 — add an auth \
         policy entry saying who it authenticates, or drop the response",
        fixture.label("openapi.json")
    ));
}

#[test]
fn reports_a_dropped_deferred_guard_binding() {
    // Reported by the source scan rather than by the policy: the guard is there
    // and the policy is satisfied, so nothing about the policy disagrees. The
    // handler simply throws the failure away, which is the bug this whole gate
    // exists for.
    let fixture = Fixture::load(UNAUTHORED);

    fixture.assert_auth_reports(&format!(
        "{}:21: get_scroll_bar: the deferred guard GuardTimestamp is bound to `auth` \
         and never enforced — `let _ = auth;` drops its failure; propagate it with \
         `?`, return it, or match on it",
        fixture.label("get/data.rs")
    ));
}

#[test]
fn reports_an_operation_in_no_policy_entry() {
    let fixture = Fixture::load(UNAUTHORED);

    fixture.assert_auth_reports(&format!(
        "{}: GET /setting (operationId `setting`) is in no auth policy entry — add one \
         naming its guards, or one marking it a public exception",
        fixture.label("openapi.json")
    ));
}

#[test]
fn reports_a_policy_entry_no_operation_answers_to() {
    let fixture = Fixture::load(UNAUTHORED);

    fixture.assert_auth_reports(&format!(
        "{}: auth policy entry `renamed_operation` names an operation the document \
         does not declare — remove the stale entry",
        fixture.label("openapi.json")
    ));
}

#[test]
fn the_unauthored_tree_reports_exactly_one_finding_per_rule() {
    let fixture = Fixture::load(UNAUTHORED);
    let data = fixture.label("get/data.rs");
    let document = fixture.label("openapi.json");

    assert_eq!(
        fixture.auth_findings(&[TEST_PREFIX], FIXTURE_POLICY),
        vec![
            format!(
                "{data}:15: data::get_rows: the auth policy requires GuardAuth but the handler declares no request guard"
            ),
            format!(
                "{data}:21: get_scroll_bar: the deferred guard GuardTimestamp is bound to `auth` and never enforced — `let _ = auth;` drops its failure; propagate it with `?`, return it, or match on it"
            ),
            format!(
                "{document}: GET /get/get-tags is guarded but documents no 401 response — add `(status = 401, response = Unauthorized)` to its #[utoipa::path]"
            ),
            format!(
                "{document}: GET /setting (operationId `setting`) is in no auth policy entry — add one naming its guards, or one marking it a public exception"
            ),
            format!(
                "{document}: GET /trashed is a public operation but documents a 401 — add an auth policy entry saying who it authenticates, or drop the response"
            ),
            format!(
                "{document}: auth policy entry `renamed_operation` names an operation the document does not declare — remove the stale entry"
            ),
        ],
        "each rule must report exactly once, and the report must stay sorted by \
         file, line and message"
    );
}

// ── Which rejections a rule expects ───────────────────────────────────────────

#[test]
fn a_read_only_mode_route_is_not_counted_as_documented_authentication() {
    // The one guard that answers 405: counting it would make every write route
    // look like it documented how it authenticates.
    let write_route = AuthRule::guarded(
        "write",
        &[GuardClass::AdminCookie, GuardClass::ReadOnlyMode],
    );
    let read_only_route = AuthRule::guarded("read_only", &[GuardClass::ReadOnlyMode]);

    assert!(write_route.documents_unauthorized());
    assert!(
        !read_only_route.documents_unauthorized(),
        "a route closed only while the server is read-only is not an authentication rule"
    );
}

#[test]
fn a_guardless_operation_says_where_its_unauthorized_comes_from() {
    assert!(!AuthRule::public("login").documents_unauthorized());
    assert!(
        AuthRule {
            operation_id: "authenticate",
            guards: &[],
            unauthenticated: openapi_sanity::Unauthenticated::CheckedByHandler,
        }
        .documents_unauthorized()
    );
    assert!(
        AuthRule {
            operation_id: "unauthorized",
            guards: &[],
            unauthenticated: openapi_sanity::Unauthenticated::LandingPage,
        }
        .documents_unauthorized()
    );
}

#[test]
fn a_security_requirement_documents_the_rejection_as_well_as_a_401_does() {
    let document: serde_json::Value = serde_json::from_str(
        r#"{
            "paths": {
                "/guarded": {
                    "get": {
                        "operationId": "guarded",
                        "security": [{ "bearer": [] }]
                    }
                },
                "/open": {
                    "get": {
                        "operationId": "open",
                        "security": []
                    }
                }
            }
        }"#,
    )
    .expect("the document is valid JSON");
    let spec = openapi_sanity::spec_operations(&document);

    assert!(spec[0].documents_rejection());
    assert!(!spec[1].documents_rejection());
}

// ── The repository's own policy ───────────────────────────────────────────────

/// The prefix the public artifact omits on purpose, as the gate is run in the
/// repository. The probes are outside the compared contract, so the policy does
/// not classify them.
const REPOSITORY_PREFIX: &str = "/get/test/";

#[test]
fn the_repository_matches_its_own_policy() {
    // The gate runs in `just check` and in CI, so the policy has to describe the
    // API as it is today. Without this, a rule that stopped comparing anything
    // would leave the fixture tests passing over a fixture while the real tree
    // went unexamined.
    let repository = repository_root();
    let router = Fixture::in_directory(&repository.join("backend").join("src").join("router"));

    assert_eq!(
        router.auth_findings_against(
            &repository.join("backend").join("openapi.json"),
            &[REPOSITORY_PREFIX],
            AUTH_POLICY
        ),
        Vec::<String>::new()
    );
}

#[test]
fn the_policy_lists_every_documented_operation_and_nothing_else() {
    // Coverage is the property the table exists for, and a count is what makes it
    // checkable: a policy that quietly drops an entry is reported by the omission
    // rule, and one that grows a duplicate by the stale rule.
    let repository = repository_root();
    let document: serde_json::Value = serde_json::from_str(&support::read(
        &repository.join("backend").join("openapi.json"),
    ))
    .expect("the committed document is valid JSON");
    let documented = openapi_sanity::spec_operations(&document)
        .into_iter()
        .filter(|operation| !operation.path.starts_with(REPOSITORY_PREFIX))
        .count();

    assert_eq!(
        AUTH_POLICY.len(),
        documented,
        "AUTH_POLICY must have exactly one entry per documented operation outside \
         the excluded prefix: {} entries for {documented} operations",
        AUTH_POLICY.len()
    );
}

#[test]
fn the_policy_claims_no_operation_twice() {
    // A repeated entry would silently shadow the earlier one: the table is looked
    // up by id, so the second rule would be the one every check used.
    let mut ids: Vec<&str> = AUTH_POLICY.iter().map(|rule| rule.operation_id).collect();
    ids.sort_unstable();
    let duplicates: Vec<&str> = ids
        .windows(2)
        .filter(|pair| pair[0] == pair[1])
        .map(|pair| pair[0])
        .collect();

    assert!(
        duplicates.is_empty(),
        "AUTH_POLICY names the same operation more than once: {duplicates:?}"
    );
}

// ── Helpers ───────────────────────────────────────────────────────────────────

impl Fixture {
    /// Assert that the auth gate reports exactly `expected`, a complete rendered
    /// diagnostic with its file and line, so a rule that moves its anchor fails
    /// here rather than still matching on wording.
    fn assert_auth_reports(&self, expected: &str) {
        let reported = self.auth_findings(&[TEST_PREFIX], FIXTURE_POLICY);

        assert!(
            reported.iter().any(|finding| finding == expected),
            "expected\n  {expected}\nfrom\n{reported:#?}"
        );
    }
}

fn repository_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the crate sits in <repo>/utils/openapi-sanity")
        .to_path_buf()
}
