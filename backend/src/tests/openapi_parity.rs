//! The route-set gate behind `--check-openapi`.
//!
//! `--check-openapi` is the load-bearing half of the `OpenAPI` invariant: it
//! compares the route table of a real `build_rocket()` with the committed
//! `backend/openapi.json`. The comparison itself is a pure function over three
//! inputs — the mounted routes, the document's operations, and the features this
//! build enables — so the rule can be driven here without a live server and
//! without a product build.
//!
//! What these tests hold is the rule's own behaviour in both directions: a
//! mounted route the document omits is a failure, a document operation this
//! build does not mount is a failure, and only a feature-gated operation whose
//! feature is disabled here is excused. The excuse branch is vacuous in
//! production today (no operation carries a marker), so it is proved here over
//! fixtures instead.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rocket::http::Method;

use crate::openapi_parity::{
    FEATURE_MARKER, KNOWN_FEATURES, Operation, compare_route_set, default_spec_path,
    documented_operations, is_outside_contract, read_spec, spec_method,
};
use crate::openapi_public::CONTRACT_EXCLUSION_PREFIXES;

/// One operation of a fixture document.
///
/// `feature` is what a `FEATURE_MARKER` extension would say about it: `None` for
/// the ungated operations that make up the repository today.
struct FixtureOperation {
    method: &'static str,
    path: &'static str,
    feature: Option<&'static str>,
}

fn ungated(method: &'static str, path: &'static str) -> FixtureOperation {
    FixtureOperation {
        method,
        path,
        feature: None,
    }
}

fn gated(method: &'static str, path: &'static str, feature: &'static str) -> FixtureOperation {
    FixtureOperation {
        method,
        path,
        feature: Some(feature),
    }
}

/// A fixture document: the shape `backend/openapi.json` has, carrying the given
/// operations and nothing else.
fn fixture_document(operations: &[FixtureOperation]) -> serde_json::Value {
    let mut paths = serde_json::Map::new();
    for operation in operations {
        let mut body = serde_json::json!({
            "operationId": operation.path.trim_matches('/').replace('/', "_"),
            "responses": {"200": {"description": "ok"}}
        });
        if let Some(feature) = operation.feature {
            body[FEATURE_MARKER] = serde_json::json!(feature);
        }
        paths
            .entry(operation.path)
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .expect("fixture path item is an object")
            .insert(operation.method.to_string(), body);
    }
    serde_json::json!({"openapi": "3.1.0", "paths": paths})
}

/// The document side of [`compare_route_set`], read through the real reader so
/// the fixtures exercise the marker parsing the check depends on.
fn documented(operations: &[FixtureOperation]) -> HashMap<Operation, Option<String>> {
    documented_operations(&fixture_document(operations)).expect("fixture document is readable")
}

/// The mount side: what Rocket would carry, already normalized by `to_spec_path`.
fn mounted(entries: &[(Method, &str)]) -> HashSet<Operation> {
    entries
        .iter()
        .map(|(method, path)| (*method, (*path).to_string()))
        .collect()
}

fn operation(method: Method, path: &str) -> Operation {
    (method, path.to_string())
}

fn rendered(report: &crate::openapi_parity::RouteSetReport) -> String {
    report.render()
}

// ── The asymmetric rule ──────────────────────────────────────────────────────

/// The load-bearing direction: a route the running product mounts and the
/// document does not carry is a hard failure, whatever the document says
/// elsewhere.
#[test]
fn a_mounted_route_the_document_omits_fails() {
    let report = compare_route_set(
        &mounted(&[(Method::Get, "/get/get-data"), (Method::Get, "/get/secret")]),
        &documented(&[ungated("get", "/get/get-data")]),
        &[],
        &CONTRACT_EXCLUSION_PREFIXES,
    );

    assert!(
        !report.is_clean(),
        "an undocumented mounted route must fail"
    );
    assert_eq!(
        report.undocumented,
        vec![operation(Method::Get, "/get/secret")]
    );
    assert_eq!(
        rendered(&report),
        "1 mounted route missing from the spec:\n  GET /get/secret"
    );
}

/// The same route is invisible to the check when the policy declares its prefix
/// outside the contract, and a prefix that only coincides is not honoured.
#[test]
fn policy_exclusions_are_dropped_before_the_comparison() {
    let report = compare_route_set(
        &mounted(&[
            (Method::Get, "/assets/{file}"),
            (Method::Get, "/get/test-but-real"),
        ]),
        &documented(&[ungated("get", "/assets/{file}")]),
        &[],
        &CONTRACT_EXCLUSION_PREFIXES,
    );

    assert!(
        !report.is_clean(),
        "a documented operation the policy excludes from the contract is not exempt from the report"
    );
    assert_eq!(
        report.undocumented,
        vec![operation(Method::Get, "/get/test-but-real")],
        "only the declared prefix is dropped; a coincidental match is reported"
    );
    assert_eq!(
        report.outside_the_contract, 2,
        "the excluded mount and the excluded document operation are both counted, so the \\
         pass/fail line cannot claim the policy dropped nothing"
    );
    assert!(is_outside_contract(
        "/assets/index.html",
        &CONTRACT_EXCLUSION_PREFIXES
    ));
    assert!(!is_outside_contract(
        "/get/test-but-real",
        &CONTRACT_EXCLUSION_PREFIXES
    ));
}

/// A document operation this build does not mount is drift unless it is
/// feature-gated and this build has the feature off. The excuse is the whole
/// reason the rule is asymmetric, and no production operation carries a marker,
/// so it is proved both ways here over fixtures.
#[test]
fn a_feature_gated_operation_absent_from_this_build_is_excused() {
    let mounted = mounted(&[(Method::Get, "/get/get-data")]);
    let documented = documented(&[
        ungated("get", "/get/get-data"),
        gated("get", "/get/index/experimental", "embed-frontend"),
    ]);

    let excused = compare_route_set(&mounted, &documented, &[], &CONTRACT_EXCLUSION_PREFIXES);
    assert!(
        excused.is_clean(),
        "a gated operation whose feature this build does not enable is expected, not drift: {}",
        rendered(&excused)
    );
    assert_eq!(
        excused.excused,
        vec![(
            operation(Method::Get, "/get/index/experimental"),
            "embed-frontend".to_string()
        )]
    );

    // The other side of the branch: the feature is on, so the operation is
    // expected to be mounted, and its absence is drift.
    let enabled = compare_route_set(
        &mounted,
        &documented,
        &enabled_this_build("embed-frontend"),
        &CONTRACT_EXCLUSION_PREFIXES,
    );
    assert!(!enabled.is_clean());
    assert_eq!(
        enabled.unmounted,
        vec![operation(Method::Get, "/get/index/experimental")]
    );
    assert!(enabled.excused.is_empty());
}

/// Without a marker there is nothing to excuse an operation with.
#[test]
fn an_ungated_document_only_operation_fails() {
    let report = compare_route_set(
        &mounted(&[(Method::Post, "/post/index/album")]),
        &documented(&[ungated("post", "/post/index/album-RENAMED")]),
        &[],
        &CONTRACT_EXCLUSION_PREFIXES,
    );

    assert!(!report.is_clean());
    assert_eq!(
        report.unmounted,
        vec![operation(Method::Post, "/post/index/album-RENAMED")]
    );
    // A renamed route is both directions at once: the mount has no operation and
    // the operation has no mount, so the report names both.
    assert_eq!(
        rendered(&report),
        "1 mounted route missing from the spec:\n  POST /post/index/album\n\n\
         1 documented operation no route mounts:\n  POST /post/index/album-RENAMED"
    );
}

/// A method is part of an operation's identity: a documented `GET` and a
/// mounted `POST` on one path are one undocumented route and one stale
/// operation, not a match.
#[test]
fn the_method_is_part_of_the_identity() {
    let report = compare_route_set(
        &mounted(&[(Method::Post, "/get/config")]),
        &documented(&[ungated("get", "/get/config")]),
        &[],
        &CONTRACT_EXCLUSION_PREFIXES,
    );

    assert_eq!(
        report.undocumented,
        vec![operation(Method::Post, "/get/config")]
    );
    assert_eq!(
        report.unmounted,
        vec![operation(Method::Get, "/get/config")]
    );
}

/// Two conforming fixtures must report nothing, and the report must be empty
/// rather than a list of zeroes.
#[test]
fn agreeing_views_report_nothing() {
    let entries = [
        (Method::Get, "/get/get-data"),
        (Method::Post, "/post/index/album"),
        (Method::Put, "/put/assign_album"),
    ];
    let documented = documented(&[
        ungated("get", "/get/get-data"),
        ungated("post", "/post/index/album"),
        ungated("put", "/put/assign_album"),
    ]);

    let report = compare_route_set(
        &mounted(&entries),
        &documented,
        &[],
        &CONTRACT_EXCLUSION_PREFIXES,
    );

    assert!(report.is_clean(), "{}", rendered(&report));
    assert_eq!(rendered(&report), "");
}

// ── Reading the document ─────────────────────────────────────────────────────

/// The marker is read off the operation object, and an operation without one is
/// not gated.
#[test]
fn the_feature_marker_is_read_from_the_operation() {
    let document = documented_operations(&fixture_document(&[
        ungated("get", "/get/get-data"),
        gated("put", "/put/gated", "embed-frontend"),
    ]))
    .expect("fixture document is readable");

    assert_eq!(document[&operation(Method::Get, "/get/get-data")], None);
    assert_eq!(
        document[&operation(Method::Put, "/put/gated")],
        Some("embed-frontend".to_string())
    );
}

/// A marker that is not a feature name, or not a string, is reported rather
/// than ignored: a marker the check cannot read is a marker that excuses
/// nothing, which is the opposite of what a broken one would do silently.
#[test]
fn an_unreadable_feature_marker_is_reported() {
    let document = fixture_document(&[ungated("get", "/get/get-data")]);
    let mut document = document;
    document["paths"]["/get/get-data"]["get"][FEATURE_MARKER] = serde_json::json!("");

    let error = documented_operations(&document).expect_err("an empty marker is not a feature");
    assert!(
        error.contains(FEATURE_MARKER) && error.contains("/get/get-data"),
        "the diagnostic must name the marker and the operation, got: {error}"
    );
}

/// A path item may carry fields that are not operations; they are not routes,
/// and an unrecognized one is reported rather than guessed at.
#[test]
fn path_item_fields_are_not_operations() {
    let document = serde_json::json!({
        "paths": {
            "/get/get-data": {
                "summary": "grid data",
                "parameters": [],
                "get": {"operationId": "get_data"}
            }
        }
    });

    let operations = documented_operations(&document).expect("a path item with fields is readable");
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[&operation(Method::Get, "/get/get-data")], None);

    let unknown = serde_json::json!({"paths": {"/get/x": {"get": {}, "x-custom": {}}}});
    assert!(
        documented_operations(&unknown).is_err(),
        "an unknown path-item key is reported, not treated as an operation"
    );
}

/// The document is read from disk, which is what makes the check a gate on the
/// committed artifact rather than on the compiled-in one.
#[test]
fn an_unreadable_spec_names_the_generator() {
    let dir = tempfile::tempdir().expect("tempdir");

    let missing = dir.path().join("openapi.json");
    let error = read_spec(&missing).expect_err("a missing spec is an error");
    assert!(
        error.contains(&missing.display().to_string()) && error.contains("just openapi-gen"),
        "the diagnostic must name the file and the fix, got: {error}"
    );

    let broken = dir.path().join("broken.json");
    std::fs::write(&broken, b"{ not json").expect("write");
    let error = read_spec(&broken).expect_err("an unparsable spec is an error");
    assert!(
        error.contains(&broken.display().to_string()) && error.contains("just openapi-gen"),
        "the diagnostic must name the file and the fix, got: {error}"
    );

    let good = dir.path().join("good.json");
    std::fs::write(
        &good,
        fixture_document(&[ungated("get", "/get/get-data")]).to_string(),
    )
    .expect("write");
    let document = read_spec(&good).expect("a readable spec parses");
    assert_eq!(documented_operations(&document).expect("readable").len(), 1);
}

/// The default is the committed artifact beside the crate manifest, so the flag
/// needs no argument in a checkout.
#[test]
fn the_default_spec_is_the_committed_artifact() {
    let path = default_spec_path();
    assert!(
        path.ends_with("backend/openapi.json"),
        "the default must be the committed artifact, got {}",
        path.display()
    );
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("openapi.json")
            .exists()
    );
}

// ── The feature set the excuse is read against ───────────────────────────────

/// A marker naming a feature the crate does not declare would make
/// `cfg!(feature = ...)` permanently false, so the operation would be excused
/// forever. The committed document is the input the check reads, so it is what
/// the pin runs over; it carries no marker today, which is why the rule is
/// proved over the fixtures above rather than here.
#[test]
fn every_feature_marker_in_the_committed_spec_is_a_declared_feature() {
    let document = read_spec(&default_spec_path()).expect("the committed spec is readable");
    let declared = declared_features();
    let operations = documented_operations(&document).expect("the committed spec is readable");

    let unknown: Vec<String> = operations
        .iter()
        .filter_map(|(operation, feature)| {
            let feature = feature.as_ref()?;
            if declared.contains(feature) {
                return None;
            }
            Some(format!(
                "{operation:?} is marked `x-picasu-feature = {feature}`"
            ))
        })
        .collect();

    assert!(
        unknown.is_empty(),
        "every feature marker must name a feature declared in backend/Cargo.toml:\n  {}",
        unknown.join("\n  ")
    );
}

/// The same pin over a document that does carry a marker, so a feature nobody
/// declared is known to be reported rather than to pass because the repository
/// happens to be clean.
#[test]
fn a_marker_naming_an_undeclared_feature_is_reported() {
    let declared = declared_features();
    let good = documented_operations(&fixture_document(&[gated(
        "get",
        "/get/x",
        "embed-frontend",
    )]))
    .expect("a declared feature is readable");
    let bad = documented_operations(&fixture_document(&[gated(
        "get",
        "/get/x",
        "embed-frontendd",
    )]))
    .expect("the marker itself parses");

    let report = |operations: &HashMap<Operation, Option<String>>| {
        operations
            .iter()
            .filter_map(|(operation, feature)| {
                let feature = feature.as_ref()?;
                (!declared.contains(feature)).then(|| format!("{operation:?} {feature}"))
            })
            .collect::<Vec<String>>()
    };

    assert!(report(&good).is_empty());
    assert_eq!(
        report(&bad),
        vec![format!(
            "{:?} embed-frontendd",
            operation(Method::Get, "/get/x")
        )]
    );
}

/// The excuse reads the features this build enables, so a declared feature the
/// reader does not name is one a marked operation is excused against
/// unconditionally. Cargo declares the features; this list is what the gate can
/// ask about, and it has to be the same set.
#[test]
fn every_declared_feature_is_readable_by_the_enabled_feature_list() {
    let declared = declared_features();
    let readable: Vec<String> = KNOWN_FEATURES
        .iter()
        .map(|feature| (*feature).to_string())
        .collect();

    for feature in &declared {
        assert!(
            readable.contains(feature),
            "feature `{feature}` is declared in backend/Cargo.toml but KNOWN_FEATURES does not \
             name it, so no build can report it enabled and a marked operation would be excused \
             in every build"
        );
    }
    for feature in &readable {
        assert!(
            declared.contains(feature),
            "KNOWN_FEATURES names `{feature}`, which no manifest declares"
        );
    }
}

/// `cfg!` cannot be evaluated at test time, so the fixture reads the same list
/// the check does, as the enabled set.
fn enabled_this_build(feature: &'static str) -> Vec<&'static str> {
    vec![feature]
}

/// The features declared in `backend/Cargo.toml`.
fn declared_features() -> Vec<String> {
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .expect("backend/Cargo.toml is readable");
    let manifest: toml::Value =
        toml::from_str(&manifest).expect("backend/Cargo.toml is a valid manifest");
    manifest
        .get("features")
        .and_then(toml::Value::as_table)
        .expect("the manifest declares a [features] table")
        .keys()
        .cloned()
        .collect()
}

#[test]
fn a_spec_method_key_is_parsed_or_reported() {
    assert_eq!(spec_method("get").expect("get"), Method::Get);
    assert_eq!(spec_method("GET").expect("an upper-case verb"), Method::Get);
    assert_eq!(spec_method("trace").expect("trace"), Method::Trace);
    assert!(spec_method("parameters").is_err());
}
