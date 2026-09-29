//! Contract parity between the mounted Rocket routes and the public OpenAPI
//! spec.
//!
//! The spec is generated from two sources that are not the runtime route table:
//! `build.rs` scans `routes![]` invocations, and utoipa reads the
//! `#[utoipa::path]` annotations. A handler can therefore be mounted and
//! documented nowhere (or documented and no longer mounted) without any build or
//! test failure. These tests compare the two views of the API directly.
//!
//! Anything intentionally outside the documented contract is listed in
//! [`is_outside_contract`] with a reason, so adding an undocumented route is a
//! deliberate, reviewable act rather than an omission.

use std::collections::{HashMap, HashSet};
use std::sync::MutexGuard;

use openapi_sanity::{AUTH_POLICY, AuthRule, check_tags};
use rocket::http::Method;

use crate::openapi_public::{is_test_only_path, public_json};
use crate::spec_path::to_spec_path;
use crate::tests::bootstrap::{TEST_ENV, TEST_SERIAL_GUARD, build_test_rocket};

/// A mounted route or a documented operation, as a comparable identity.
type Operation = (Method, String);

/// Parse a spec method key. `Method` has no fallible parser, and an unknown
/// verb should fail the gate rather than be skipped.
fn spec_method(key: &str) -> Method {
    match key.to_ascii_lowercase().as_str() {
        "get" => Method::Get,
        "post" => Method::Post,
        "put" => Method::Put,
        "delete" => Method::Delete,
        other => panic!("spec declares unsupported HTTP method {other}"),
    }
}

/// Every route mounted on the application, in spec path form.
fn mounted_operations() -> HashSet<Operation> {
    build_test_rocket()
        .routes()
        .map(|route| (route.method, to_spec_path(&route.uri.to_string())))
        .collect()
}

/// Every operation in the public spec, in spec path form.
fn spec_operations() -> HashSet<Operation> {
    let spec: serde_json::Value =
        serde_json::from_str(&public_json()).expect("public spec must be valid JSON");
    let paths = spec["paths"].as_object().expect("spec paths object");
    let mut operations = HashSet::new();
    for (path, item) in paths {
        for method in item.as_object().expect("path item object").keys() {
            operations.insert((spec_method(method), path.clone()));
        }
    }
    operations
}

/// Mounted routes that are deliberately absent from the documented contract.
fn is_outside_contract(operation: &Operation) -> bool {
    let (method, path) = operation;
    // Test-only probes: registered only in test builds, enabled only by the
    // test bootstrap, and stripped from the public spec on purpose.
    if is_test_only_path(path) {
        return true;
    }
    // Static file server for the built frontend. It serves bytes, not API
    // operations, so it carries no OpenAPI operation.
    *method == Method::Get && path.starts_with("/assets")
}

/// Mounted routes that are in scope of the contract but not documented.
fn undocumented_routes(mounted: &HashSet<Operation>, documented: &HashSet<Operation>) -> String {
    render(
        &mounted
            .difference(documented)
            .filter(|operation| !is_outside_contract(operation))
            .cloned()
            .collect(),
    )
}

/// Operations in the spec that no route serves any more.
fn stale_operations(documented: &HashSet<Operation>, mounted: &HashSet<Operation>) -> String {
    render(&documented.difference(mounted).cloned().collect())
}

/// `operationId` values claimed by more than one path.
fn duplicate_operation_ids(spec: &serde_json::Value) -> Vec<String> {
    let mut seen: HashMap<&str, &str> = HashMap::new();
    let mut duplicates = Vec::new();
    for (path, item) in spec["paths"].as_object().expect("spec paths object") {
        for operation in item.as_object().expect("path item object").values() {
            let id = operation["operationId"]
                .as_str()
                .expect("every operation declares an operationId");
            if let Some(previous) = seen.insert(id, path) {
                duplicates.push(format!("{id}: {previous} and {path}"));
            }
        }
    }
    duplicates.sort_unstable();
    duplicates
}

fn render(operations: &HashSet<Operation>) -> String {
    let mut lines: Vec<String> = operations
        .iter()
        .map(|(method, path)| format!("{method} {path}"))
        .collect();
    lines.sort_unstable();
    lines.join("\n")
}

/// Guards against a scenario mutating `APP_CONFIG` or wiping the data path while
/// the route table is read.
fn lock_state() -> MutexGuard<'static, ()> {
    let _ = &*TEST_ENV;
    TEST_SERIAL_GUARD.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn every_mounted_route_is_documented() {
    let _guard = lock_state();
    let mounted = mounted_operations();
    let documented = spec_operations();

    let undocumented = undocumented_routes(&mounted, &documented);

    assert!(
        undocumented.is_empty(),
        "mounted routes missing from the public spec:\n{}\n\
         Fix: add a `#[utoipa::path]` annotation to the handler and make sure its \
         module is scanned by `backend/build.rs`.",
        undocumented
    );
}

#[test]
fn every_spec_operation_is_mounted() {
    let _guard = lock_state();
    let stale = stale_operations(&spec_operations(), &mounted_operations());

    assert!(
        stale.is_empty(),
        "operations documented in the public spec but not mounted:\n{}\n\
         Fix: the route was renamed or removed — regenerate the spec with \
         `just openapi-gen` and review the diff.",
        stale
    );
}

#[test]
fn operation_ids_are_unique() {
    let _guard = lock_state();
    let spec: serde_json::Value =
        serde_json::from_str(&public_json()).expect("public spec must be valid JSON");

    let duplicates = duplicate_operation_ids(&spec);
    assert!(
        duplicates.is_empty(),
        "duplicate operationIds break generated client method names:\n{}",
        duplicates.join("\n")
    );
}

/// Keeps the exclusion list honest: an entry that no longer matches a mounted
/// route is dead configuration, and a documented route that is still excluded
/// means the exclusion is hiding real API surface.
#[test]
fn contract_exclusions_match_mounted_routes() {
    let _guard = lock_state();
    let mounted = mounted_operations();
    let documented = spec_operations();
    let excluded: Vec<Operation> = mounted
        .iter()
        .filter(|operation| is_outside_contract(operation))
        .cloned()
        .collect();

    assert!(
        !excluded.is_empty(),
        "no mounted route is excluded from the contract — if the probes and the \
         file server are now documented, remove the exclusions instead"
    );
    for operation in excluded {
        assert!(
            !documented.contains(&operation),
            "{} {} is excluded from the contract but documented in the public \
             spec; remove it from `is_outside_contract`",
            operation.0,
            operation.1
        );
    }
}

/// The Rocket-to-`OpenAPI` path translation is owned by this crate
/// (`crate::spec_path`), and the mounted-route comparison above runs on the
/// same function: a second copy would be free to drift, and a route that
/// stopped matching its own documentation would go unnoticed. These cases pin
/// the whole mapping — segment declarations, underscores, dots, the query part
/// and malformed input.
#[test]
fn rocket_paths_normalize_to_spec_templates() {
    // Named segments become placeholders.
    assert_eq!(
        to_spec_path("/get/metadata/<asset_id>"),
        "/get/metadata/{asset_id}"
    );
    // A zero-or-more segment drops the dots.
    assert_eq!(
        to_spec_path("/object/compressed/<file_path..>"),
        "/object/compressed/{file_path}"
    );
    // `<_path..>` exists to avoid a clash with the handler name; OpenAPI has no
    // counterpart for the underscore.
    assert_eq!(
        to_spec_path("/albums/view/<_path..>"),
        "/albums/view/{path}"
    );
    assert_eq!(to_spec_path("/assets/<_file..>"), "/assets/{file}");
    assert_eq!(
        to_spec_path("/albums/view/<_path..>/photos/<index>/raw"),
        "/albums/view/{path}/photos/{index}/raw"
    );
    // Query parameters are documented per parameter, not in the path.
    assert_eq!(to_spec_path("/get/prefetch?<locate>"), "/get/prefetch");
    assert_eq!(
        to_spec_path("/upload?<auto_rename>&<on_conflict>"),
        "/upload"
    );
    // A path without parameters is unchanged.
    assert_eq!(to_spec_path("/upload"), "/upload");
    assert_eq!(to_spec_path("/login"), "/login");
    assert_eq!(to_spec_path(""), "");
    // Not a segment declaration: the remainder is passed through rather than
    // silently dropped, so a malformed URI is visible in the comparison.
    assert_eq!(to_spec_path("/get/<broken"), "/get/<broken");
}

// ── Operation tags ────────────────────────────────────────────────────────────
//
// Every operation carries exactly one tag from a small taxonomy so the generated
// reference groups by subject instead of by whichever handler was annotated last.
// The vocabulary and the rules live in `openapi_sanity` (`KNOWN_TAGS`,
// `check_tags`), and `openapi-sanity check` runs them on the committed document.
// What follows is the document half of that run, kept in the backend so
// `cargo test --lib` fails on the same drift without the CLI. It reads the
// *generated* public spec rather than the committed artifact, so an annotation
// edit is reported before `just openapi-gen` has run, while the CLI reports what
// the committed document says and phase 2 of the gate catches the two disagreeing.

/// The label the shared checks print for the committed artifact.
const PUBLIC_SPEC_LABEL: &str = "backend/openapi.json";

/// Every way an operation's tags violate the shared taxonomy, rendered as the
/// shared check reports it.
///
/// The rules, the vocabulary and the `pages` placement logic belong to
/// `openapi_sanity::check_tags`; what the backend reads is the generated public
/// spec rather than the committed document, which is the one view of the
/// contract only this crate has.
fn tag_violations(spec: &serde_json::Value) -> Vec<String> {
    let operations = openapi_sanity::spec_operations(spec);

    check_tags(PUBLIC_SPEC_LABEL, &operations, &[])
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// The taxonomy gate: every operation in the public spec is grouped by a tag from
/// the shared vocabulary, and `pages` sits on the SPA page routes and nowhere
/// else.
#[test]
fn the_public_operations_follow_the_shared_tag_taxonomy() {
    assert_eq!(
        tag_violations(&public_spec()),
        Vec::<String>::new(),
        "operation tags must follow the taxonomy in `docs/openapi-generator.md`. \
         Fix: set `tag = \"...\"` in the handler's `#[utoipa::path]` annotation and \
         run `just openapi-gen`."
    );
}

/// The vocabulary is a hand-written claim, and the shared check is the only thing
/// that holds the document to it here. A check that stopped comparing would leave
/// this test passing over a document nothing was read from, so the taxonomy is
/// required to notice a tag the annotation does not have.
#[test]
fn self_check_detects_tag_drift_in_the_public_spec() {
    let mut retagged = public_spec();
    retagged["paths"]["/get/get-data"]["get"]["tags"] = serde_json::json!(["pages"]);

    assert_eq!(
        tag_violations(&public_spec()),
        Vec::<String>::new(),
        "the unmutated document is the baseline this self-check is measured against"
    );
    let violations = tag_violations(&retagged);

    assert_eq!(
        violations,
        vec![format!(
            "{PUBLIC_SPEC_LABEL}: GET /get/get-data: data-API path carries the `pages` tag"
        )],
        "a data operation wearing the SPA page tag is the drift the reserved tag \
         exists to catch, and it has to be reported where the vocabulary lives now: \
         {violations:?}"
    );
}

// ── Negative self-checks ──────────────────────────────────────────────────────
//
// The gates above only fail if the comparison still works. A refactor that
// emptied `undocumented_routes`, or a path-normalization change that silently
// made every route "match", would turn the contract gate into a no-op that
// still reports success. These tests feed the comparison deliberately drifted
// inputs and require it to notice, so the gate cannot be neutered silently.

/// Build an operation set from `(method, path)` pairs.
fn operations(entries: &[(Method, &str)]) -> HashSet<Operation> {
    entries
        .iter()
        .map(|(method, path)| (*method, (*path).to_string()))
        .collect()
}

#[test]
fn self_check_detects_an_undocumented_mounted_route() {
    let mounted = operations(&[
        (Method::Get, "/get/get-data"),
        (Method::Post, "/post/undeclared"),
    ]);
    let documented = operations(&[(Method::Get, "/get/get-data")]);

    let undetected = undocumented_routes(&mounted, &documented);
    assert!(
        undetected.contains("POST /post/undeclared"),
        "a mounted route missing from the spec must be reported, got: {undetected:?}"
    );
}

#[test]
fn self_check_detects_a_documented_operation_that_is_not_mounted() {
    // What a stale `path = "..."` in an annotation looks like: documented,
    // mounted under a different path.
    let mounted = operations(&[(Method::Post, "/post/index/album")]);
    let documented = operations(&[(Method::Post, "/post/index/album-RENAMED")]);

    let undetected = stale_operations(&documented, &mounted);
    assert!(
        undetected.contains("POST /post/index/album-RENAMED"),
        "a spec operation with no matching route must be reported, got: {undetected:?}"
    );
}

#[test]
fn self_check_reports_nothing_when_both_views_agree() {
    let entries = [
        (Method::Get, "/get/get-data"),
        (Method::Post, "/post/renew-hash-token"),
        (Method::Put, "/put/assign_album"),
    ];
    let mounted = operations(&entries);
    let documented = operations(&entries);

    assert_eq!(undocumented_routes(&mounted, &documented), "");
    assert_eq!(stale_operations(&documented, &mounted), "");
}

#[test]
fn self_check_does_not_report_excluded_routes_as_undocumented() {
    // If the exclusions were removed from the comparison, the probes and the
    // file server would be reported as drift on every run. This asserts the
    // filter is doing the work, and that the exclusions are still narrow.
    let mounted = operations(&[
        (Method::Get, "/get/test/record/{asset_id}"),
        (Method::Get, "/assets/{path}"),
        (Method::Post, "/post/real-route"),
    ]);
    let documented = operations(&[(Method::Post, "/post/real-route")]);

    let undetected = undocumented_routes(&mounted, &documented);
    assert_eq!(
        undetected, "",
        "test-only and file-server routes must stay out of the contract report"
    );
    // ...and an excluded prefix must not swallow a real route.
    let real_route = (Method::Get, "/get/test-but-real".to_string());
    assert!(!is_outside_contract(&real_route));
}

#[test]
fn self_check_detects_duplicate_operation_ids() {
    let spec = serde_json::json!({
        "paths": {
            "/get/get-data": {"get": {"operationId": "get_data"}},
            "/get/get-rows": {"get": {"operationId": "get_data"}},
            "/get/get-tags": {"get": {"operationId": "get_tags"}}
        }
    });

    let duplicates = duplicate_operation_ids(&spec);
    assert_eq!(
        duplicates,
        vec!["get_data: /get/get-data and /get/get-rows"]
    );

    let unique = duplicate_operation_ids(&serde_json::json!({
        "paths": {"/get/get-data": {"get": {"operationId": "get_data"}}}
    }));
    assert!(unique.is_empty());
}

#[test]
fn self_check_detects_a_method_mismatch() {
    // Same path, different verb: `GET /get/config` documented while only
    // `POST /get/config` is mounted. Path-only comparison would miss it.
    let mounted = operations(&[(Method::Post, "/get/config")]);
    let documented = operations(&[(Method::Get, "/get/config")]);

    assert!(undocumented_routes(&mounted, &documented).contains("POST /get/config"));
    assert!(stale_operations(&documented, &mounted).contains("GET /get/config"));
}

// ── Shared 401 response component ─────────────────────────────────────────────
//
// Every operation that can answer 401 must reference the single
// `Unauthorized` component registered by `backend/build.rs`, so the meaning of
// a 401 is documented once instead of drifting per route.
//
// Which operations can answer 401 is not decided here. It is
// `openapi_sanity::AUTH_POLICY`, one entry per documented operation, and
// `openapi-sanity check` holds the source to it from two sides: the handler has to
// declare the guard the entry names, and the document has to state the rejection.
// What follows is the document half of that, kept in the backend so
// `cargo test --lib` fails on the same drift without running the CLI — the
// limitation being that it can only read the document, not the handler
// signature, so it is weaker than the check it duplicates.

/// Parse the public spec.
fn public_spec() -> serde_json::Value {
    serde_json::from_str(&public_json()).expect("public spec must be valid JSON")
}

/// The operation object for `(method, path)`, or `Null` when undocumented.
fn operation<'a>(spec: &'a serde_json::Value, method: &str, path: &str) -> &'a serde_json::Value {
    &spec["paths"][path][method.to_ascii_lowercase()]
}

/// `GET /unauthorized` is the landing page whose own response body is a 401,
/// not an API authentication failure, so it keeps an inline response instead of
/// the shared component. Named explicitly rather than exempted by tag, so a new
/// page operation cannot bypass the rule.
fn is_unauthorized_landing_page(method: &str, path: &str) -> bool {
    method == "get" && path == "/unauthorized"
}

/// The `401` response entry of an operation, or `Null` when it declares none.
fn unauthorized_response<'a>(operation: &'a serde_json::Value) -> &'a serde_json::Value {
    &operation["responses"]["401"]
}

#[test]
fn spec_registers_the_shared_unauthorized_response() {
    let spec = public_spec();
    let unauthorized = spec["components"]["responses"]["Unauthorized"]
        .as_object()
        .expect(
            "components.responses.Unauthorized is not registered — `backend/build.rs` must emit \
             `components(responses(Unauthorized))` and import `crate::openapi_components::Unauthorized`",
        );
    let description = unauthorized
        .get("description")
        .and_then(|d| d.as_str())
        .unwrap_or_default();
    assert!(
        !description.is_empty(),
        "the Unauthorized component must describe when a 401 occurs"
    );
    assert!(
        spec["paths"]["/get/get-data"]["get"]["responses"]["401"]["$ref"]
            == "#/components/responses/Unauthorized",
        "the `$ref` target must match the registered component name"
    );
}

#[test]
fn unauthorized_response_is_referenced_not_inlined() {
    let spec = public_spec();
    let paths = spec["paths"].as_object().expect("spec paths object");
    let mut violations = Vec::new();

    for (path, item) in paths {
        for (method, operation) in item.as_object().expect("path item object") {
            let unauthorized = unauthorized_response(operation);
            if unauthorized.is_null() {
                continue;
            }
            if is_unauthorized_landing_page(method, path) {
                continue;
            }
            let expected = serde_json::json!({
                "$ref": "#/components/responses/Unauthorized"
            });
            if unauthorized != &expected {
                violations.push(format!(
                    "{method} {path}: 401 must be `{expected}`, got `{unauthorized}`"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "operations must reference the shared Unauthorized component instead of \
         inlining a duplicate response literal:\n{}",
        violations.join("\n")
    );
}

/// Every documented operation, keyed by the `operationId` the auth policy is
/// indexed by.
///
/// The policy is a property of the API, not of this document, so it is written in
/// operation ids; resolving an entry to a route is what this map is for, and it is
/// also how a stale entry is found: a name nothing declares resolves to nothing.
fn operations_by_id(spec: &serde_json::Value) -> HashMap<&str, Operation> {
    let mut by_id = HashMap::new();
    for (path, item) in spec["paths"].as_object().expect("spec paths object") {
        for (method, operation) in item.as_object().expect("path item object") {
            if let Some(id) = operation["operationId"].as_str() {
                by_id.insert(id, (spec_method(method), path.clone()));
            }
        }
    }
    by_id
}

/// Every way the auth policy and the document disagree about who can answer 401.
///
/// Three directions, because each one alone is satisfied by a policy that has
/// drifted: an entry for an operation that no longer exists, an operation the
/// policy protects that does not document the rejection, and an operation
/// documenting a rejection the policy does not account for. Pure, so a neutered
/// comparison fails a self-check below rather than passing quietly.
fn policy_violations(spec: &serde_json::Value, policy: &[AuthRule]) -> Vec<String> {
    let documented = operations_by_id(spec);
    let mut violations = Vec::new();

    for rule in policy {
        let Some((method, path)) = documented.get(rule.operation_id) else {
            violations.push(format!(
                "AUTH_POLICY entry `{}` names an operation the spec does not declare",
                rule.operation_id
            ));
            continue;
        };
        let declares = !unauthorized_response(operation(spec, &method.to_string(), path)).is_null();
        if declares == rule.documents_unauthorized() {
            continue;
        }
        if rule.documents_unauthorized() {
            violations.push(format!(
                "{method} {path}: the auth policy says it can answer 401 but it documents none"
            ));
        } else {
            violations.push(format!(
                "{method} {path}: documents a 401 the auth policy does not account for"
            ));
        }
    }

    let accounted: HashSet<Operation> = policy
        .iter()
        .filter(|rule| rule.documents_unauthorized())
        .filter_map(|rule| documented.get(rule.operation_id).cloned())
        .collect();
    for (path, item) in spec["paths"].as_object().expect("spec paths object") {
        for (method, operation) in item.as_object().expect("path item object") {
            if unauthorized_response(operation).is_null() {
                continue;
            }
            if !accounted.contains(&(spec_method(method), path.clone())) {
                violations.push(format!(
                    "{method} {path}: documents a 401 no auth policy entry accounts for"
                ));
            }
        }
    }

    violations
}

#[test]
fn the_auth_policy_and_the_documented_unauthorized_responses_agree() {
    assert_eq!(
        policy_violations(&public_spec(), AUTH_POLICY),
        Vec::<String>::new(),
        "the auth policy and the generated spec have to agree on which operations can \
         answer 401"
    );
}

#[test]
fn self_check_detects_a_protected_operation_without_a_documented_401() {
    let mut without = public_spec();
    without["paths"]["/get/get-data"]["get"]["responses"]
        .as_object_mut()
        .expect("responses object")
        .remove("401");

    assert!(
        policy_violations(&public_spec(), AUTH_POLICY).is_empty(),
        "the unmutated document is the baseline the self-checks are measured against"
    );
    let violations = policy_violations(&without, AUTH_POLICY);

    assert!(
        violations
            .iter()
            .any(|violation| violation.starts_with("GET /get/get-data:")),
        "dropping the 401 from a protected operation must be reported: {violations:?}"
    );
    assert!(
        policy_violations(&public_spec(), AUTH_POLICY).is_empty(),
        "and the violation has to be the mutation's, not the document's"
    );
}

#[test]
fn self_check_detects_a_stale_auth_policy_entry() {
    let policy: Vec<AuthRule> = AUTH_POLICY
        .iter()
        .copied()
        .chain(std::iter::once(AuthRule::public("renamed_operation")))
        .collect();

    assert_eq!(
        policy_violations(&public_spec(), &policy),
        vec![
            "AUTH_POLICY entry `renamed_operation` names an operation the spec does not \
             declare"
                .to_string()
        ]
    );
}

#[test]
fn self_check_detects_a_public_operation_that_documents_a_401() {
    // A public page made to answer 401 is the drift the reverse direction exists
    // for: the route was closed, or the response is stale, and the policy is what
    // says which.
    let mut spec = public_spec();
    spec["paths"]["/login"]["get"]["responses"]["401"] =
        serde_json::json!({ "$ref": "#/components/responses/Unauthorized" });

    let violations = policy_violations(&spec, AUTH_POLICY);

    assert!(
        violations
            .iter()
            .any(|violation| violation.starts_with("GET /login:")),
        "a public operation documenting a 401 must be reported: {violations:?}"
    );
}
