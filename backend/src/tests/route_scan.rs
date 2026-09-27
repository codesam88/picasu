//! The backend's side of the route scanner.
//!
//! The scanner itself — the `routes![...]` entries, the per-function
//! `#[utoipa::path]` attribution and the Rocket-to-`OpenAPI` path translation —
//! lives in `utils/openapi-sanity` and is unit-tested there, because a build
//! script cannot be tested in place. What is left for the backend to answer is
//! which router files `build.rs` scans, and that it scans them through the
//! shared crate rather than a private copy.

use std::path::Path;

fn build_script() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("build.rs");
    std::fs::read_to_string(&path).expect("build.rs is readable")
}

/// Guards the scanner against the modules that carry the real route tables:
/// dropping one from the scan list unmounts nothing but silently documents
/// nothing, which is the drift the contract gate reports.
#[test]
fn scanned_router_modules_cover_every_mounted_group() {
    let source = build_script();

    for module in [
        "get/mod.rs",
        "post/mod.rs",
        "put/mod.rs",
        "delete.rs",
        "auth.rs",
    ] {
        assert!(
            source.contains(&format!("\"{module}\"")),
            "build.rs no longer scans {module}; its routes would be mounted but \
             undocumented"
        );
    }
}

/// The scanner must come from the shared library. A private copy inside the
/// build script is how the generator and the contract tests end up comparing
/// route tables with two different notions of what a path is.
#[test]
fn build_script_scans_with_the_shared_analyzer() {
    let source = build_script();

    assert!(
        source.contains("openapi_sanity::scan_routes"),
        "build.rs no longer reads `routes![]` blocks with the shared analyzer"
    );
    assert!(
        source.contains("openapi_sanity::scan_handlers"),
        "build.rs no longer reads `#[utoipa::path]` annotations with the shared analyzer"
    );
    assert!(
        !source.contains("mod route_scan"),
        "build.rs carries its own route scanner again; use `openapi-sanity` instead"
    );
}
