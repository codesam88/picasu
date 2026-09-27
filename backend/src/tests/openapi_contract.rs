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

use openapi_sanity::{AUTH_POLICY, AuthRule, to_spec_path};
use rocket::http::Method;

use crate::openapi_public::{is_test_only_path, public_json};
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
    // Test-only probes: mounted in every build, enabled only by the test
    // bootstrap, and stripped from the public spec on purpose.
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

/// The Rocket-to-`OpenAPI` path translation is owned by `openapi-sanity`, which
/// unit-tests it. What is asserted here is that both this test and the mounted
/// route comparison above run on that shared implementation: a local copy would
/// be free to drift, and a route that stopped matching its own documentation
/// would go unnoticed.
#[test]
fn rocket_paths_normalize_to_spec_templates() {
    assert_eq!(
        to_spec_path("/get/metadata/<asset_id>"),
        "/get/metadata/{asset_id}"
    );
    assert_eq!(
        to_spec_path("/albums/view/<_path..>"),
        "/albums/view/{path}"
    );
    assert_eq!(
        to_spec_path("/object/compressed/<file_path..>"),
        "/object/compressed/{file_path}"
    );
    // Query parameters are documented per parameter, not in the path.
    assert_eq!(to_spec_path("/get/prefetch?<locate>"), "/get/prefetch");
    assert_eq!(to_spec_path("/upload"), "/upload");
}

// ── Operation tags ────────────────────────────────────────────────────────────
//
// Every operation carries exactly one tag from a small taxonomy so the
// generated reference groups by subject instead of by whichever handler was
// annotated last. The taxonomy and its meaning are documented in
// `docs/openapi-generator.md`; the comparison is a pure function so the
// negative self-checks below can feed it drifted input.

/// Every tag the taxonomy allows. Adding a tag is a deliberate one-line change
/// here plus the matching row in `docs/openapi-generator.md`.
const KNOWN_TAGS: &[&str] = &[
    "albums", "assets", "auth", "config", "index", "pages", "serving", "timeline", "upload",
];

/// Path shapes that identify data-API operations, as opposed to SPA HTML pages.
const DATA_API_PREFIXES: &[&str] = &["/delete/", "/get/", "/object/", "/post/", "/put/"];

/// Whether `path` is a data-API operation rather than an SPA page route.
///
/// Derived from path shape because the spec alone does not say which file
/// annotated an operation. Stated assumption: every public-spec operation that
/// is *not* under one of these prefixes (or `POST /upload`) is one of the
/// `router/get/get_page.rs` HTML routes. The two things that would break the
/// assumption — the test-only probes and the `/assets` file server — are
/// excluded from the public spec. If a data route were ever added outside
/// these shapes, this function would classify it as a page and the tag gate
/// would fail loudly instead of accepting the wrong grouping.
fn is_data_api_path(path: &str) -> bool {
    DATA_API_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
        || path == "/upload"
}

/// Report every way a spec's operation tags can violate the taxonomy: an
/// operation with no tags, a tag outside [`KNOWN_TAGS`], a data-API path
/// carrying `pages`, and a page path missing `pages`. Pure — it reads only the
/// JSON it is given, so self-checks can feed it deliberately drifted input.
fn tag_violations(spec: &serde_json::Value) -> String {
    let mut violations = Vec::new();
    for (path, item) in spec["paths"].as_object().expect("spec paths object") {
        for (method, operation) in item.as_object().expect("path item object") {
            let id = format!("{method} {path}");
            let tags: Vec<&str> = operation["tags"]
                .as_array()
                .map(|entries| entries.iter().filter_map(|t| t.as_str()).collect())
                .unwrap_or_default();
            if tags.is_empty() {
                violations.push(format!("{id}: declares no tags"));
            }
            for tag in &tags {
                if !KNOWN_TAGS.contains(tag) {
                    violations.push(format!("{id}: unknown tag `{tag}`"));
                }
            }
            let data_api = is_data_api_path(path);
            if data_api && tags.contains(&"pages") {
                violations.push(format!("{id}: data-API path carries the `pages` tag"));
            }
            if !data_api && !tags.contains(&"pages") {
                violations.push(format!("{id}: SPA page path must carry `pages`"));
            }
        }
    }
    violations.join("\n")
}

/// The taxonomy gate: every operation in the public spec is grouped by exactly
/// the known tags, and `pages` sits on the SPA page routes and nowhere else.
#[test]
fn every_operation_carries_a_known_tag() {
    let report = tag_violations(&public_spec());

    assert!(
        report.is_empty(),
        "operation tags violate the taxonomy (documented in \
         docs/openapi-generator.md):\n{report}\n\
         Fix: set `tag = \"...\"` in the handler's `#[utoipa::path]` annotation \
         and run `just openapi-gen`.",
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

#[test]
fn self_check_detects_an_untagged_operation() {
    // What dropping `tag = "..."` from an annotation looks like: the operation
    // serializes without a usable `tags` array.
    let spec = serde_json::json!({
        "paths": {
            "/get/get-data": {"get": {"tags": ["timeline"]}},
            "/put/edit_tag": {"put": {}}
        }
    });

    let report = tag_violations(&spec);
    assert!(
        report.contains("put /put/edit_tag: declares no tags"),
        "an operation with no tags must be reported, got: {report:?}"
    );
    assert!(
        !report.contains("/get/get-data"),
        "a conforming operation must not be reported, got: {report:?}"
    );
}

#[test]
fn self_check_detects_an_unknown_tag() {
    // A tag outside the taxonomy: present, so the missing-tag rule cannot
    // catch it, but not in `KNOWN_TAGS`.
    let spec = serde_json::json!({
        "paths": {"/put/edit_tag": {"put": {"tags": ["metadata"]}}}
    });

    let report = tag_violations(&spec);
    assert!(
        report.contains("put /put/edit_tag: unknown tag `metadata`"),
        "a tag outside KNOWN_TAGS must be reported, got: {report:?}"
    );
}

#[test]
fn self_check_detects_a_data_api_path_tagged_pages() {
    // The mistake this gate exists to prevent: a data-API operation wearing
    // the SPA page tag, which randomizes the reference grouping again.
    let spec = serde_json::json!({
        "paths": {"/get/get-data": {"get": {"tags": ["pages"]}}}
    });

    let report = tag_violations(&spec);
    assert!(
        report.contains("get /get/get-data: data-API path carries the `pages` tag"),
        "a data-API operation tagged `pages` must be reported, got: {report:?}"
    );
}

#[test]
fn self_check_detects_a_page_path_without_the_pages_tag() {
    // The other direction of the `pages` rule: an SPA page route that lost
    // (or never got) its tag must be reported too.
    let spec = serde_json::json!({
        "paths": {"/login": {"get": {"tags": ["auth"]}}}
    });

    let report = tag_violations(&spec);
    assert!(
        report.contains("get /login: SPA page path must carry `pages`"),
        "an untagged-by-shape page operation must be reported, got: {report:?}"
    );
}

#[test]
fn self_check_accepts_a_conformant_tag_set() {
    let spec = serde_json::json!({
        "paths": {
            "/get/get-data": {"get": {"tags": ["timeline"]}},
            "/login": {"get": {"tags": ["pages"]}},
            "/upload": {"post": {"tags": ["upload"]}}
        }
    });

    assert_eq!(
        tag_violations(&spec),
        "",
        "a spec that follows the taxonomy must report no violations"
    );
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
