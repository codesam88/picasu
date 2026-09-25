//! Unit tests for the shared Rocket→OpenAPI path translation in
//! `backend/build/route_path.rs`, plus the test that pins its two call
//! sites — the build-time annotation check and the mounted-route parity
//! gate — to the same behaviour on the real tree.

// The translation under test; the same module `build.rs` and
// `src/tests/openapi_contract.rs` include. Named `translation` rather than
// `route_path` so the include does not nest a module inside its own name.
#[path = "../../build/route_path.rs"]
mod translation;

use std::collections::BTreeSet;
use std::path::Path;

use crate::tests::ast_scan::declared_rocket_uris;
use crate::tests::openapi_contract::is_outside_contract_path;
use translation::to_spec_path;

#[test]
fn translates_rocket_parameter_forms_to_spec_templates() {
    assert_eq!(
        to_spec_path("/get/metadata/<asset_id>"),
        "/get/metadata/{asset_id}"
    );
    // The leading underscore only exists to keep the Rust binding from
    // clashing with the handler name; it is not part of the path.
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
    assert_eq!(to_spec_path("/get/get-data?<start>&<end>"), "/get/get-data");
    assert_eq!(to_spec_path("/upload"), "/upload");
    assert_eq!(to_spec_path("/"), "/");
}

#[test]
fn malformed_parameter_is_kept_verbatim_without_panicking() {
    // An unterminated `<` is not a segment declaration; the remainder comes
    // back recognisable instead of panicking — `build.rs` must never abort
    // the build over a route URI it cannot translate.
    assert_eq!(to_spec_path("/a/<b"), "/a/<b");
    assert_eq!(to_spec_path("<"), "<");
    assert_eq!(to_spec_path("/a?"), "/a");
    assert_eq!(to_spec_path(""), "");
}

/// The Rocket side of the spec-agreement test: every URI declared under
/// `src/router`, discovered by the AST pass (see
/// `tests::ast_scan::declared_rocket_uris`), translated to spec form.
fn declared_router_paths() -> BTreeSet<String> {
    declared_rocket_uris()
        .iter()
        .map(|uri| to_spec_path(uri))
        .collect()
}

/// The path keys of the committed `backend/openapi.json`.
fn committed_spec_paths() -> BTreeSet<String> {
    let spec_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("openapi.json");
    let text = std::fs::read_to_string(&spec_path).expect("committed spec is readable");
    let spec: serde_json::Value =
        serde_json::from_str(&text).expect("committed spec is valid JSON");
    spec["paths"]
        .as_object()
        .expect("spec declares a paths object")
        .keys()
        .cloned()
        .collect()
}

/// Pins the translation's two call sites to the same behaviour: every Rocket
/// path declared in the router must translate to a path the committed spec
/// declares, and every spec path must be reachable from some declaration —
/// the same two strings the parity gate compares, derived here from source
/// rather than the runtime mount table. If a future route declares a URI
/// whose translation would differ from the spec, this fails.
#[test]
fn router_paths_translate_to_the_committed_spec_paths() {
    let declared = declared_router_paths();
    assert!(
        declared.len() > 50,
        "expected the router's route declarations, found {}",
        declared.len()
    );
    let documented = committed_spec_paths();

    // Deliberate non-contract paths (test-only probes, the static file
    // server) use the parity gate's own exclusion list, so an exclusion
    // removed there stops applying here too.
    let undocumented: Vec<&String> = declared
        .difference(&documented)
        .filter(|path| !is_outside_contract_path(path))
        .collect();
    let stale: Vec<&String> = documented.difference(&declared).collect();

    assert!(
        undocumented.is_empty(),
        "Rocket paths whose translation is absent from backend/openapi.json: \
         {undocumented:?}"
    );
    assert!(
        stale.is_empty(),
        "spec paths no Rocket declaration translates to: {stale:?}"
    );
}
