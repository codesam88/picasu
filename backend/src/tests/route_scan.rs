//! The backend's side of the route scanner.
//!
//! The scanner itself — the `routes![...]` entries, the per-function
//! `#[utoipa::path]` attribution, the Rocket-to-`OpenAPI` path translation and
//! the source/spec contract checks — lives in `utils/openapi-sanity` and is
//! tested there. What is left for the backend to answer is which router files
//! are part of the contract, that those files exist, and that the build script
//! reads them through the shared crate rather than a private copy.

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
    for module in [
        "get/mod.rs",
        "post/mod.rs",
        "put/mod.rs",
        "delete.rs",
        "auth.rs",
    ] {
        assert!(
            openapi_sanity::SCANNED_MODULES
                .iter()
                .any(|(_, relative)| *relative == module),
            "{module} is no longer scanned; its routes would be mounted but undocumented"
        );
    }
}

/// Every module of the scan list must be a file that exists. The generator skips
/// a module it cannot read, so a stale entry there shrinks the contract without
/// a word in the build log — the renewal routes were once documented this way,
/// and a deleted router module leaves the same kind of hole.
#[test]
fn every_scanned_router_module_exists() {
    let router = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("router");

    for (group, relative) in openapi_sanity::SCANNED_MODULES {
        let path = router.join(relative);
        assert!(
            path.is_file(),
            "SCANNED_MODULES lists {relative} (group `{group}`), but {} does not exist",
            path.display()
        );
    }
}

/// The group prefix of a scanned module must match the group its file is laid
/// out under, because that is how a `routes![]` entry resolves to a handler
/// file. A mismatch here makes every handler of the module unresolvable.
#[test]
fn scanned_router_module_groups_match_their_layout() {
    for (group, relative) in openapi_sanity::SCANNED_MODULES {
        let unit = openapi_sanity::SourceUnit::for_relative_path(relative, relative, "");
        assert_eq!(
            unit.group_prefix(),
            *group,
            "SCANNED_MODULES says group `{group}` for {relative}"
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
