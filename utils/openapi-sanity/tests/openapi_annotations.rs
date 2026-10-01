//! Source-level checks on the `#[utoipa::path]` annotations.
//!
//! This increment covers the two rules of section C that are about the handler
//! body — **C1** (a `GuardResult<…>` argument must have its rejection
//! propagated) and **C1b** (a plain `Guard…` argument needs nothing in the body
//! and is never reported). The annotation-shape, parameter-agreement and
//! security rules are separate increments.
//!
//! Why these two belong in the gate. `GuardResult<T>` is `Result<T, AppError>`:
//! the route hands the handler a value that may be a rejection, and the
//! *handler* is the only place that rejection can become an error response. Drop
//! it and the route serves a request the guard refused — the `84f29aa5` shape,
//! which an earlier, now-deleted analyzer — the crate this tool is named after —
//! caught through a hand-written `AUTH_POLICY` table. A plain `GuardAuth` is the
//! opposite case: Rocket runs it during request handling and short-circuits on
//! failure, so the handler legitimately never touches the value. Treating the two
//! alike would report every handler that correctly binds one as broken.
//!
//! Both rules read a fact that exists only in source — nothing in the generated
//! document or in the mount table says what a handler body does with a guard —
//! so they are a source check that runs as its own phase of `just
//! openapi-check`, rather than a build warning nothing reads. The rules live in
//! the crate's `lib.rs` so the CLI and these tests check the same code.
//!
//! Fixtures live in `tests/fixtures/openapi_annotations/` as ordinary `.rs`
//! files. They are deliberately *not* modules of the crate: they are snippets for
//! the parser, not code to compile, so nothing declares them. Each is pulled in
//! with `include_str!`, so a renamed or deleted fixture breaks the build instead
//! of quietly skipping a test.
//!
//! Every rule has a fixture that must produce its finding, a conforming
//! counterpart that must produce none, and a run over the real backend router
//! tree that must stay silent. Removing a check fails a named test rather than
//! turning the gate green.
//!
//! Two shapes are not named by C1 and had no precedent in the tree. They are
//! pinned as tests below rather than left to be inferred from the walker, and
//! neither expectation may be changed without a rule change going through review:
//!
//! - A guard moved into a closure and propagated there —
//!   `spawn_blocking(move || { … auth?; … })` — is **accepted**: the walker
//!   recurses, so the `?` inside the closure is an occurrence in a recognised
//!   position.
//! - A guard rebound one hop — `let carried = auth; … carried?;` — is
//!   **reported**: every propagating position requires the binding itself to
//!   appear, and a rebinding is not one. The rejection does reach the caller, so
//!   this finding may well be wrong; it is pinned as-is because deciding that is
//!   a rule change, not a fixture edit.

use std::path::{Path, PathBuf};

use openapi_sanity::{
    Finding, Requirement, findings_in_source, guard_requirement, handlers_in_file, render,
    scan_source_root,
};

/// Both rules over one source file, for the fixtures below.
///
/// A fixture that does not parse is a broken test, not a finding, so the error
/// ends the test here rather than being reported as one.
fn check_source(name: &str, source: &str) -> Vec<Finding> {
    findings_in_source(name, source).unwrap_or_else(|error| panic!("{error}"))
}

// ── Fixtures ──────────────────────────────────────────────────────────────────

const DISCARDED: &str = include_str!("fixtures/openapi_annotations/c1_discarded_guard_result.rs");
const ABSENT: &str = include_str!("fixtures/openapi_annotations/c1_absent_guard_result.rs");
const CONFORMING: &str = include_str!("fixtures/openapi_annotations/c1_conforming.rs");
const PLAIN_GUARDS: &str = include_str!("fixtures/openapi_annotations/c1b_plain_guards.rs");
const UNANNOTATED: &str = include_str!("fixtures/openapi_annotations/c1_unannotated_ignored.rs");
const MOVED_INTO_CLOSURE: &str =
    include_str!("fixtures/openapi_annotations/c1_moved_into_closure.rs");
const REBOUND: &str = include_str!("fixtures/openapi_annotations/c1_rebound_guard_result.rs");

/// The mutation fixture of C1: a handler binding two fallible guards, dropping
/// one without `?` and consuming the other correctly. Only the first is a
/// finding — a rule that flagged both would be rejecting the house idiom.
#[test]
fn dropping_a_guard_result_fails() {
    let findings = check_source("c1_discarded_guard_result.rs", DISCARDED);

    assert_eq!(
        render(&findings),
        "c1_discarded_guard_result.rs:13: dropped_guard_result: the route binds \
         GuardResult<GuardAuth> as a fallible guard, but the handler body uses it \
         without ever propagating the rejection, so the guard's rejection never \
         reaches the caller",
        "the handler drops one guard and propagates the other; only the drop is a finding"
    );
}

/// The same rule from the other side: a binding the body never touches cannot be
/// propagated, and rustc says nothing about an unused argument.
#[test]
fn a_guard_result_absent_from_the_body_fails() {
    let findings = check_source("c1_absent_guard_result.rs", ABSENT);

    assert_eq!(
        render(&findings),
        "c1_absent_guard_result.rs:9: unused_guard_result: the route binds \
         GuardResult<GuardAuth> as a fallible guard, but the handler body never \
         mentions it, so the guard's rejection never reaches the caller"
    );
}

/// The conforming counterpart for C1: `?`, `match`, `if let`, forwarded to a
/// call, `return`ed, and returned as the trailing expression. None of these may
/// be reported, or the rule would reject the idiom the tree uses everywhere.
#[test]
fn propagated_guard_results_are_accepted() {
    let findings = check_source("c1_conforming.rs", CONFORMING);

    assert_eq!(
        render(&findings),
        "",
        "every recognised propagating position must be silent"
    );
}

/// The rule applies to annotated handlers only. An undocumented route is a
/// contract finding reported elsewhere; raising it here too would send the
/// reader to a body check when the problem is a missing annotation.
#[test]
fn a_dropped_guard_in_an_unannotated_handler_is_out_of_scope() {
    let findings = check_source("c1_unannotated_ignored.rs", UNANNOTATED);

    assert_eq!(render(&findings), "");
}

// ── Ambiguity cases, pinned as decisions ──────────────────────────────────────
//
// Two shapes are not named by the rule and had no precedent in the tree. They
// are recorded here as tests so the behaviour is a stated decision rather than
// something a later reader has to infer from the walker. Changing either
// expectation is a rule change and belongs in review, not in a fixture edit.

/// A fallible guard moved into a closure and propagated inside it — the shape
/// `tokio::task::spawn_blocking(move || { … auth?; … })` takes.
///
/// The walker recurses through `Visit`, so the `?` inside the closure body is an
/// occurrence of the binding in a recognised position and the handler is
/// **silent**. That is the behaviour today, and this test is what pins it: if a
/// future edit stops the recursion from reaching closure bodies, this fails
/// instead of the rule quietly narrowing.
#[test]
fn guard_moved_into_a_closure_is_accepted() {
    let findings = check_source("c1_moved_into_closure.rs", MOVED_INTO_CLOSURE);

    assert_eq!(
        render(&findings),
        "",
        "the `?` inside the closure body propagates the rejection, so the binding is \
         not discarded"
    );
}

/// A fallible guard rebound one hop: `let carried = auth;` then `carried?;`.
///
/// Every propagating position the rule recognises requires the *binding* to
/// appear as `ident?`, as a scrutinee, as a call argument or as a returned value.
/// A rebinding is none of those — `auth` only ever turns up as the initialiser of
/// a `let` — so the rule reports it as a discard. **That is the behaviour today,
/// and it is pinned as-is:** the guard's rejection does reach the caller, so a
/// reviewer may well decide the finding is wrong, but that is a rule change and
/// not something this fixture should quietly decide.
#[test]
fn rebound_guard_result_is_reported() {
    let findings = check_source("c1_rebound_guard_result.rs", REBOUND);

    assert_eq!(
        render(&findings),
        "c1_rebound_guard_result.rs:17: rebound_guard_result: the route binds \
         GuardResult<GuardAuth> as a fallible guard, but the handler body uses it \
         without ever propagating the rejection, so the guard's rejection never \
         reaches the caller",
        "the binding is rebound before the `?`, and the rule reports the rebinding \
         rather than following the new name; see the module doc for why this is \
         pinned rather than accepted"
    );
}

/// C1b as an enforced statement rather than an intention.
///
/// Two things have to hold: an unused plain guard produces no finding, and it is
/// classified as [`Requirement::None`] rather than merely happening not to match
/// a pattern. Asserting the classification is what stops a later edit from
/// widening guard detection into the plain guards and turning five correct
/// handlers into findings.
#[test]
fn an_unused_plain_guard_is_accepted() {
    let handlers =
        handlers_in_file("c1b_plain_guards.rs", PLAIN_GUARDS).expect("fixture must parse as Rust");

    let unused = handlers
        .iter()
        .find(|handler| handler.name == "unused_plain_guard")
        .expect("fixture keeps its unused-plain-guard handler");
    assert_eq!(
        unused.guards.len(),
        2,
        "both plain guards are recognised as guards"
    );
    assert!(
        unused
            .guards
            .iter()
            .all(|guard| guard.requirement == Requirement::None),
        "a plain guard obliges the body to nothing: {:?}",
        unused.guards
    );

    let mixed = handlers
        .iter()
        .find(|handler| handler.name == "mixed_guards")
        .expect("fixture keeps its mixed-guards handler");
    let classified: Vec<(&str, &str, Requirement)> = mixed
        .guards
        .iter()
        .map(|guard| {
            (
                guard.ident.as_str(),
                guard.guard_type.as_str(),
                guard.requirement,
            )
        })
        .collect();
    assert_eq!(
        classified,
        vec![
            (
                "auth",
                "GuardResult<GuardAuth>",
                Requirement::PropagateRejection
            ),
            ("share", "GuardShare", Requirement::None),
        ],
        "only the `GuardResult` binding carries an obligation; the plain `GuardShare` \
         is Rocket's to run"
    );

    let findings = check_source("c1b_plain_guards.rs", PLAIN_GUARDS);
    assert_eq!(
        render(&findings),
        "",
        "Rocket short-circuits a plain guard, so an unused one is correct code"
    );
}

/// The classification pinned on types rather than on a fixture: a type that is
/// not a guard at all carries no obligation for any guard rule, so a new guard
/// type cannot fail the build before someone has decided what it means.
#[test]
fn only_a_guard_result_carries_an_obligation() {
    let guard_result: syn::Type = syn::parse_quote!(GuardResult<GuardAuth>);
    let plain: syn::Type = syn::parse_quote!(GuardAuth);
    let not_a_guard: syn::Type = syn::parse_quote!(Json<AppConfig>);

    assert_eq!(
        guard_requirement(&guard_result),
        Some(Requirement::PropagateRejection)
    );
    assert_eq!(guard_requirement(&plain), Some(Requirement::None));
    assert_eq!(guard_requirement(&not_a_guard), None);
}

// ── The real tree ─────────────────────────────────────────────────────────────

/// How many `#[utoipa::path]` annotations the router tree carries today.
///
/// Pinned rather than derived, so a scan that silently stops finding annotations
/// — a broken file walk, a swallowed parse error — fails here instead of
/// reporting a clean tree it never looked at. Update it with the annotation.
const ANNOTATIONS_IN_ROUTER: usize = 63;

/// The guard inventory the rules were calibrated against: 52 fallible bindings,
/// all consumed as `let _ = ident?;`, and 9 plain `GuardAuth` bindings, none of
/// which the body touches — Rocket runs them and short-circuits on failure.
const GUARD_INVENTORY: (usize, usize) = (52, 9);

/// The backend's router tree, resolved from this crate's manifest directory
/// rather than from the working directory a test happens to run in.
fn router_tree() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../backend/src/router")
}

/// Both rules over the real router tree.
///
/// This is the half that proves the rules against the tree rather than against
/// snippets, and the half that pins the scan's coverage so the two cannot drift
/// apart silently.
#[test]
fn guard_propagation_is_clean_across_the_router_tree() {
    let report = scan_source_root(&router_tree()).expect("the router tree must be readable");
    assert!(
        report.files_scanned > 1,
        "the scan must see the whole router tree, saw {} file(s)",
        report.files_scanned
    );

    let fallible_guards: usize = report
        .handlers
        .iter()
        .map(|handler| handler.fallible_guards)
        .sum();
    let plain_guards: usize = report
        .handlers
        .iter()
        .map(|handler| handler.plain_guards)
        .sum();

    assert_eq!(
        report.handlers.len(),
        ANNOTATIONS_IN_ROUTER,
        "the scan must see every annotation in backend/src/router"
    );
    assert_eq!(
        (fallible_guards, plain_guards),
        GUARD_INVENTORY,
        "the tree's guard inventory moved; these rules were calibrated against it"
    );
    assert_eq!(
        render(&report.findings),
        "",
        "every `GuardResult` binding must propagate its rejection:\n{}",
        render(&report.findings)
    );
}
