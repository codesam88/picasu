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

use rocket::http::Method;

use crate::openapi_public::{is_test_only_path, public_json};
use crate::tests::bootstrap::{TEST_ENV, TEST_SERIAL_GUARD, build_test_rocket};

/// A mounted route or a documented operation, as a comparable identity.
type Operation = (Method, String);

/// Rewrite a Rocket route URI to OpenAPI path-template form.
///
/// Rocket declares segments as `<name>` / `<name..>`; OpenAPI uses `{name}`. A
/// leading underscore in a Rocket segment name (`<_path..>`, used to avoid a
/// clash with the handler name) has no OpenAPI counterpart and is dropped. The
/// query part is dropped because it is documented per parameter, not in the path.
fn to_spec_path(uri: &str) -> String {
    let path = uri.split('?').next().unwrap_or(uri);
    let mut out = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('>') else {
            // Not a segment declaration; keep the remainder verbatim.
            out.push_str(&rest[start..]);
            return out;
        };
        out.push('{');
        out.push_str(after[..end].trim_end_matches('.').trim_start_matches('_'));
        out.push('}');
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

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
