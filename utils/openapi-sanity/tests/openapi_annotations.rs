//! Source-level checks on the `#[utoipa::path]` annotations.
//!
//! Two sections of `.plan/openapi-annotation-checks.md` are checked here: **A**,
//! the six rules about the annotation's own shape, and the two rules of section C
//! that are about the handler body — **C1** (a `GuardResult<…>` argument must have
//! its rejection propagated) and **C1b** (a plain `Guard…` argument needs nothing
//! in the body and is never reported). The parameter-agreement (B) and security
//! (D, M) rules are separate increments.
//!
//! Why these belong in the gate. Each of them reads a fact that exists only in
//! source, and each is invisible in the document: the document is generated *from*
//! the annotations, so it cannot disagree with them, and the route table is
//! assembled by a macro. A restated path, a missing `responses(…)`, a tag outside
//! the vocabulary, a handler with no doc comment, a summary wrapped over two lines
//! and a hand-set `operation_id` all produce a document that looks complete while
//! carrying a wrong, missing or unsortable field — and the doc comment case
//! produced 49 operations with no `summary` before the rule existed. The guard
//! case is the other direction: `GuardResult<T>` is `Result<T, AppError>`, the
//! route hands the handler a value that may be a rejection, and the *handler* is
//! the only place that rejection can become an error response. Drop it and the
//! route serves a request the guard refused — the `84f29aa5` shape, which an
//! earlier, now-deleted analyzer — the crate this tool is named after — caught
//! through a hand-written `AUTH_POLICY` table. A plain `GuardAuth` is the opposite
//! case: Rocket runs it during request handling and short-circuits on failure, so
//! the handler legitimately never touches the value. Treating the two alike would
//! report every handler that correctly binds one as broken.
//!
//! So the rules run as their own phase of `just openapi-check` rather than as a
//! build warning nothing reads, and they live in the crate's `lib.rs` so the CLI
//! and these tests check the same code.
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
    Finding, Requirement, TAGS, findings_in_source, guard_requirement, handlers_in_file, render,
    scan_source_root,
};

/// Every rule over one source file, for the fixtures below.
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

const RESTATED_ROUTE: &str = include_str!("fixtures/openapi_annotations/a1_restated_route.rs");
const ROUTE_ONLY: &str = include_str!("fixtures/openapi_annotations/a1_conforming.rs");
const NO_RESPONSES: &str = include_str!("fixtures/openapi_annotations/a2_missing_responses.rs");
const RESPONSES_DECLARED: &str = include_str!("fixtures/openapi_annotations/a2_conforming.rs");
const BAD_TAGS: &str =
    include_str!("fixtures/openapi_annotations/a3_tag_outside_the_vocabulary.rs");
const VOCABULARY_TAGS: &str = include_str!("fixtures/openapi_annotations/a3_conforming.rs");
const NO_DOC_COMMENT: &str = include_str!("fixtures/openapi_annotations/a4_missing_doc_comment.rs");
const DOC_COMMENTED: &str = include_str!("fixtures/openapi_annotations/a4_conforming.rs");
const SPLIT_SUMMARY: &str = include_str!("fixtures/openapi_annotations/a5_multi_line_summary.rs");
const ONE_LINE_SUMMARY: &str = include_str!("fixtures/openapi_annotations/a5_conforming.rs");
const HAND_SET_ID: &str = include_str!("fixtures/openapi_annotations/a6_hand_set_operation_id.rs");
const DERIVED_ID: &str = include_str!("fixtures/openapi_annotations/a6_conforming.rs");
const HAND_SET_PROSE: &str = include_str!("fixtures/openapi_annotations/a7_hand_set_prose.rs");
const DERIVED_PROSE: &str = include_str!("fixtures/openapi_annotations/a7_conforming.rs");

// ── Section A — the annotation's shape ─────────────────────────────────────────
//
// Every rule has a fixture that must produce its finding and a conforming
// counterpart that must produce none. The conforming fixture is not decoration:
// a rule satisfied by flagging every annotation would pass the first test and is
// caught by the second.
//
// The findings are pinned as whole rendered strings, line included, so a rule
// that starts reporting somewhere else — or stops reporting at all — fails a
// named test rather than changing the gate's output quietly.

/// A1: `rocket_extras` derives the path and the verb from the route attribute,
/// so either restated in the annotation is a second copy of a fact nothing
/// compares.
#[test]
fn a_restated_path_or_verb_fails() {
    let findings = check_source("a1_restated_route.rs", RESTATED_ROUTE);

    assert_eq!(
        render(&findings),
        "a1_restated_route.rs:8: restated_path: the annotation declares path = \
         \"/get/widget\", but rocket_extras derives the path from the route \
         attribute, so a restatement can only be a duplicate that can rot\na1_restated_route.rs:18: \
         restated_verb: the annotation names the verb get as a bare argument, but \
         rocket_extras derives the verb from the route attribute, so a restatement \
         can only be a duplicate that can rot\na1_restated_route.rs:32: restated_trace_verb: \
         the annotation names the verb trace as a bare argument, but rocket_extras \
         derives the verb from the route attribute, so a restatement can only be a \
         duplicate that can rot",
        "each restatement is a finding, at the line it is written on; `trace` is in the \
         list because utoipa accepts it as a bare verb token"
    );
}

/// The conforming counterpart: the same two handlers with the annotation saying
/// only what the route attribute cannot.
#[test]
fn a_route_annotation_free_of_restatement_is_accepted() {
    let findings = check_source("a1_conforming.rs", ROUTE_ONLY);

    assert_eq!(
        render(&findings),
        "",
        "an annotation that repeats neither the path nor the verb must be silent"
    );
}

/// A2: utoipa invents no response, so an absent `responses(…)` documents nothing
/// the operation can answer. An empty `responses()` is the same defect and is
/// reported as its own text, because it reads as though responses were considered.
#[test]
fn a_missing_or_empty_responses_fails() {
    let findings = check_source("a2_missing_responses.rs", NO_RESPONSES);

    assert_eq!(
        render(&findings),
        "a2_missing_responses.rs:7: no_responses: the annotation declares no responses, \
         and utoipa invents no response, so the operation documents nothing it can \
         answer\na2_missing_responses.rs:12: empty_responses: the annotation declares \
         responses() with no entry, and utoipa invents no response, so the operation \
         documents nothing it can answer",
        "the absent declaration and the empty one are both findings, and the absent \
         one is anchored at the signature because there is no token to point at"
    );
}

/// The conforming counterpart: one response and two responses are both enough.
#[test]
fn one_declared_response_is_enough() {
    let findings = check_source("a2_conforming.rs", RESPONSES_DECLARED);

    assert_eq!(
        render(&findings),
        "",
        "the rule asks that the operation document what it can answer, not every status"
    );
}

/// A3: the tag is what the generated reference groups by, and the vocabulary is
/// the closed list in `docs/openapi-generator.md` ("Tag conventions").
///
/// All three failure shapes are in one fixture: no tag, a tag outside the
/// vocabulary, and two tags of which both are inside it.
#[test]
fn a_tag_outside_the_vocabulary_fails() {
    let findings = check_source("a3_tag_outside_the_vocabulary.rs", BAD_TAGS);

    assert_eq!(
        render(&findings),
        "a3_tag_outside_the_vocabulary.rs:8: no_tag: the annotation declares no tag, so \
         the operation is filed nowhere in the generated reference; take one from the \
         vocabulary in docs/openapi-generator.md \"Tag conventions\"\n\
         a3_tag_outside_the_vocabulary.rs:14: unknown_tag: the annotation declares the tag \
         \"data\", which is not one of the 10 in docs/openapi-generator.md \"Tag \
         conventions\" (albums, assets, auth, config, index, internal, pages, serving, \
         timeline, upload); a tag outside the vocabulary files the operation outside every \
         section of the reference\n\
         a3_tag_outside_the_vocabulary.rs:24: two_tags: the annotation declares 2 tags \
         (\"assets\", \"timeline\"), but the house rule is exactly one tag per operation, \
         so the reference would file it under all of them",
        "a missing tag, an unknown tag and a repeated tag are three different defects"
    );
}

/// Every tag of the vocabulary is accepted, so A3 is a closed list and not a
/// preference for the tags that happen to be in use.
///
/// The fixture cannot carry all nine without becoming a wall of text, so the
/// second half of this test builds its source from [`TAGS`] itself: a tag added
/// to the vocabulary is then accepted by construction, and a tag *removed* from
/// it fails, which is the direction that matters. Without it, a tenth tag could
/// be added to the constant and stay unchecked by the suite.
#[test]
fn every_tag_of_the_vocabulary_is_accepted() {
    let fixture = check_source("a3_conforming.rs", VOCABULARY_TAGS);
    assert_eq!(
        render(&fixture),
        "",
        "one tag of the vocabulary is enough, and `auth` is one of them"
    );

    let source = TAGS
        .iter()
        .enumerate()
        .map(|(index, tag)| {
            format!(
                "/// Widget page {index}.\n#[utoipa::path(\n    tag = \"{tag}\",\n    \
                 responses((status = 200, description = \"Ok\"))\n)]\n\
                 #[get(\"/get/widget-{index}\")]\n\
                 pub async fn widget_{index}() -> AppResult<Json<Widget>> {{\n    Ok(Json(Widget))\n}}\n"
            )
        })
        .collect::<String>();

    let findings = check_source("every_tag.rs", &source);
    assert_eq!(
        render(&findings),
        "",
        "every tag in the vocabulary must be accepted:\n{}",
        render(&findings)
    );
}

/// A4: `summary` and `description` are derived from the doc comment, so a
/// handler without one is an operation with no text in the reference — and the
/// document still looks complete.
#[test]
fn a_handler_without_a_doc_comment_fails() {
    let findings = check_source("a4_missing_doc_comment.rs", NO_DOC_COMMENT);

    assert_eq!(
        render(&findings),
        "a4_missing_doc_comment.rs:10: undocumented_widget: the handler carries no doc \
         comment, and summary and description are derived from it, so the operation \
         reaches the generated reference with neither",
        "an annotated handler with nothing above it documents nothing in the reference"
    );
}

/// The conforming counterpart: a doc comment in any shape satisfies A4. The shape
/// of its first paragraph is A5's business, not this rule's.
#[test]
fn a_doc_commented_handler_is_accepted() {
    let findings = check_source("a4_conforming.rs", DOC_COMMENTED);

    assert_eq!(
        render(&findings),
        "",
        "A4 asks for a comment, not for a particular one"
    );
}

/// A5: utoipa derives `summary` from the doc comment's first paragraph and
/// `widdershins` renders the summary as the reference's heading, so a paragraph
/// of more than one line puts a newline inside a markdown heading.
///
/// This defect was measured in this repository's own document: six of its
/// operations had a multi-line summary before the rule existed.
#[test]
fn a_multi_line_summary_fails() {
    let findings = check_source("a5_multi_line_summary.rs", SPLIT_SUMMARY);

    assert_eq!(
        render(&findings),
        "a5_multi_line_summary.rs:7: multi_line_summary: the doc comment's first paragraph \
         must be one line, and it is the operation's summary, which the reference renders \
         as a heading; a heading must be one line, and this paragraph is 2 line(s)",
        "the finding is anchored at the second line of the paragraph, which is where the \
         heading stops being a heading"
    );
}

/// The conforming counterpart: a one-line first paragraph followed by a blank doc
/// line and a description wrapped over several. Only the first paragraph is the
/// summary, so wrapping the rest is what paragraphs are for.
#[test]
fn a_one_line_summary_with_a_wrapped_description_is_accepted() {
    let findings = check_source("a5_conforming.rs", ONE_LINE_SUMMARY);

    assert_eq!(
        render(&findings),
        "",
        "the rule bounds the summary, not the whole doc comment"
    );
}

/// A6: utoipa derives `operation_id` from the function name, and every other name
/// in the document is derived the same way or compared against the mount table.
/// A hand-set one is the name nothing compares.
#[test]
fn a_hand_set_operation_id_fails() {
    let findings = check_source("a6_hand_set_operation_id.rs", HAND_SET_ID);

    assert_eq!(
        render(&findings),
        "a6_hand_set_operation_id.rs:8: widget: the annotation sets operation_id = \
         \"getWidget\", which utoipa otherwise derives from the function name; a hand-set \
         one is the only name in the document that nothing compares",
        "a hand-set operation id is a finding, at the line it is written on"
    );
}

/// The conforming counterpart: no `operation_id`, so the name is derived and
/// every consumer of it compares it.
#[test]
fn a_derived_operation_id_is_accepted() {
    let findings = check_source("a6_conforming.rs", DERIVED_ID);

    assert_eq!(render(&findings), "");
}

/// A7: utoipa derives `summary` from the doc comment's first paragraph and
/// `description` from the rest, so a hand-set one is the same prose twice, with
/// nothing comparing the copies.
///
/// A7 is what makes A5's premise true. While an annotation may set `summary`,
/// "the first paragraph *is* the summary" does not hold, and A5 would be
/// reporting a defect the document does not have.
#[test]
fn a_hand_set_summary_or_description_fails() {
    let findings = check_source("a7_hand_set_prose.rs", HAND_SET_PROSE);

    assert_eq!(
        render(&findings),
        "a7_hand_set_prose.rs:9: hand_set_prose: the annotation sets summary = \"Move a \
         widget into an album\", which utoipa otherwise derives from the doc comment; a \
         hand-set one is prose written twice beside itself, and nothing compares the two \
         copies\na7_hand_set_prose.rs:10: hand_set_prose: the annotation sets description \
         = \"Moves the widget into the album directory on disk.\", which utoipa otherwise \
         derives from the doc comment; a hand-set one is prose written twice beside itself, \
         and nothing compares the two copies",
        "each override is its own finding, at the line it is written on"
    );
}

/// The conforming counterpart: the doc comment says all of it, and the
/// per-response `description` is left alone — it is how a status code's text is
/// written, not something utoipa derives.
#[test]
fn a_derived_summary_and_description_are_accepted() {
    let findings = check_source("a7_conforming.rs", DERIVED_PROSE);

    assert_eq!(
        render(&findings),
        "",
        "a per-response description is not a hand-set operation description"
    );
}

/// The mutation fixture of C1: a handler binding two fallible guards, dropping
/// one without `?` and consuming the other correctly. Only the first is a
/// finding — a rule that flagged both would be rejecting the house idiom.
#[test]
fn dropping_a_guard_result_fails() {
    let findings = check_source("c1_discarded_guard_result.rs", DISCARDED);

    assert_eq!(
        render(&findings),
        "c1_discarded_guard_result.rs:14: dropped_guard_result: the route binds \
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
        "c1_absent_guard_result.rs:10: unused_guard_result: the route binds \
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
        "c1_rebound_guard_result.rs:18: rebound_guard_result: the route binds \
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

/// Every rule over the real router tree, which has to be silent.
///
/// This is the half that proves the rules against the tree rather than against
/// snippets, and the half that pins the scan's coverage so the two cannot drift
/// apart silently. Its name does not name a rule on purpose: a test called after
/// one rule would read as coverage of that rule when it is the run that says
/// whether the whole rule set holds.
#[test]
fn the_router_tree_is_clean() {
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
        "every rule must hold across the tree:\n{}",
        render(&report.findings)
    );
}
