//! The scan the annotation rules will be reported through, checked against the
//! real router tree.
//!
//! This increment is the tool rather than a rule set. The rules of
//! `.plan/openapi-annotation-checks.md` land in later increments and live in the
//! crate's `lib.rs`, so the CLI and these tests check the same code.
//!
//! What is pinned here is the walk's coverage rather than a rule's verdict. A
//! scan that silently stopped finding annotations — a broken file walk, a
//! swallowed parse error — would run every later rule over a tree it never
//! looked at while still reporting clean, so the count is pinned against the
//! real tree now, while there is nothing else it could hide behind.

use std::path::{Path, PathBuf};

use openapi_sanity::{render, scan_source_root};

// ── The real tree ─────────────────────────────────────────────────────────────

/// How many `#[utoipa::path]` annotations the router tree carries today.
///
/// Pinned rather than derived, so a scan that silently stops finding annotations
/// — a broken file walk, a swallowed parse error — fails here instead of
/// reporting a clean tree it never looked at. Update it with the annotation.
const ANNOTATIONS_IN_ROUTER: usize = 63;

/// The backend's router tree, resolved from this crate's manifest directory
/// rather than from the working directory a test happens to run in.
fn router_tree() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../backend/src/router")
}

/// The walk over the real router tree.
///
/// This is the half that pins the scan's coverage, so the rules added later
/// cannot pass by running over a tree the walk never reached.
#[test]
fn the_scan_sees_every_annotation_in_the_router_tree() {
    let report = scan_source_root(&router_tree()).expect("the router tree must be readable");

    assert!(
        report.files_scanned > 1,
        "the scan must see the whole router tree, saw {} file(s)",
        report.files_scanned
    );
    assert_eq!(
        report.handlers.len(),
        ANNOTATIONS_IN_ROUTER,
        "the scan must see every annotation in backend/src/router"
    );
    assert_eq!(
        render(&report.findings),
        "",
        "no rule is implemented yet, so the tree must be silent:\n{}",
        render(&report.findings)
    );
}
