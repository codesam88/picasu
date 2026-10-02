//! Source-level checks on the `#[utoipa::path]` annotations.
//!
//! Four sections of `.plan/openapi-annotation-checks.md` are checked here: **A**,
//! the eight rules about the annotation's own shape, **B**, the four rules about
//! what the annotation declares against what the route already says, the two
//! rules of section C that are about the handler body — **C1** (a
//! `GuardResult<…>` argument must have its rejection propagated) and **C1b** (a
//! plain `Guard…` argument needs nothing in the body and is never reported) —
//! and **C3**, what a guard obliges the document to say. The security (D) rules
//! and M1/M3/M4 are separate increments; the mode guard's `405`, which the plan
//! called **M2**, is C3's `GuardReadOnlyMode` row rather than a rule of its own.
//!
//! Two rules read what a guard *says* rather than what it *does*: **A8**, which
//! asks what a guard binding is called, and **C3**, which asks whether the
//! annotation declares the status that guard rejects with. Both read the same
//! binding C1 does, and both cross-fix the other's fixtures, so that the findings
//! in `a8_guard_binding_named_after_another_class.rs` are A8's alone and the
//! findings in `c3_guard_rejection_status_missing.rs` are C3's alone.
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
//! **A8 has two branches and both are exercised.** The second exists because Rocket
//! binds `?<timestamp>` to a handler argument of the same name, so
//! `GuardTimestamp` cannot take its canonical name in any signature in this
//! repository. Where the canonical name is taken the binding must still carry it as
//! a word-part; where it is taken *and* the binding carries none of it, that is a
//! finding — which is what keeps the branch from being an exemption. The four
//! bindings that take the branch are counted and pinned.
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
    Finding, GUARD_CLASSES, GuardClass, Requirement, TAGS, findings_in_source, guard_class,
    guard_requirement, handlers_in_file, render, scan_source_root,
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

const UNBOUND_PARAMETER: &str =
    include_str!("fixtures/openapi_annotations/b1_parameter_the_route_does_not_bind.rs");
const BOUND_PARAMETERS: &str = include_str!("fixtures/openapi_annotations/b1_conforming.rs");
const DISAGREEING_OPTIONALITY: &str =
    include_str!("fixtures/openapi_annotations/b2_optionality_the_argument_disagrees_with.rs");
const AGREEING_OPTIONALITY: &str = include_str!("fixtures/openapi_annotations/b2_conforming.rs");
const UNPARSED_BODY: &str =
    include_str!("fixtures/openapi_annotations/b3_body_the_route_does_not_parse.rs");
const PARSED_BODY: &str = include_str!("fixtures/openapi_annotations/b3_conforming.rs");
const FORM_WITHOUT_MULTIPART: &str =
    include_str!("fixtures/openapi_annotations/b4_form_body_without_multipart.rs");
const FORM_WITH_MULTIPART: &str = include_str!("fixtures/openapi_annotations/b4_conforming.rs");

const MISNAMED_GUARD_BINDINGS: &str =
    include_str!("fixtures/openapi_annotations/a8_guard_binding_named_after_another_class.rs");
const CANONICAL_NAME_TAKEN: &str =
    include_str!("fixtures/openapi_annotations/a8_canonical_name_taken.rs");
const GUARD_BINDINGS_AFTER_CLASS: &str =
    include_str!("fixtures/openapi_annotations/a8_conforming.rs");
const MISSING_REJECTION_STATUS: &str =
    include_str!("fixtures/openapi_annotations/c3_guard_rejection_status_missing.rs");
const DOCUMENTED_REJECTION_STATUS: &str =
    include_str!("fixtures/openapi_annotations/c3_conforming.rs");

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

// ── Section B — what the annotation declares against what the route says ───────
//
// The invariant of this section is that **the route wins**: `rocket_extras` reads
// the route attribute and supplies the path, the verb, a parameter for every
// argument the signature binds, and a request body for `data = "…"`. What the
// annotation declares is merged on top and compared against nothing, so each of
// these rules reads one declaration and one fact the route already states.
//
// A rule here that read only the annotation would be a tautology — utoipa derives
// `required` from the declared type — so every fixture pairs a declared parameter
// or body with a route that disagrees with it.

/// B1: `rocket_extras` derives a parameter for every argument the route binds,
/// and merges whatever the annotation declares on top. Nothing checks the other
/// direction, so a declared parameter no route reads reaches the document.
#[test]
fn a_parameter_the_route_does_not_bind_fails() {
    let findings = check_source("b1_parameter_the_route_does_not_bind.rs", UNBOUND_PARAMETER);

    assert_eq!(
        render(&findings),
        "b1_parameter_the_route_does_not_bind.rs:17: prefetch: the annotation declares the \
         query parameter \"nope\", but the route binds no such query parameter: its query \
         part is \"?<locate>\"\n\
         b1_parameter_the_route_does_not_bind.rs:35: get_asset: the annotation declares the \
         path parameter \"album_id\", but the route binds no such path parameter: its path is \
         \"/assets/<asset_id>\"",
        "both locations are checked, each against the part of the route that binds it, and \
         the parameters the route *does* bind in the same annotations are silent"
    );
}

/// The conforming counterpart: a query parameter and a path parameter the route
/// binds, a route with no query part at all, and a partial-segment name — the
/// `..` is Rocket's marker and not part of the name the handler binds.
#[test]
fn parameters_the_route_binds_are_accepted() {
    let findings = check_source("b1_conforming.rs", BOUND_PARAMETERS);

    assert_eq!(
        render(&findings),
        "",
        "a declared parameter the route binds must be silent, including a `<name..>` \
         partial segment"
    );
}

/// B2: utoipa 5.5 has no `required` key in a parameter tuple and derives the
/// documented `required` from the declared type, so the two optionalities that
/// have to agree are the declared type's and the handler argument's.
///
/// Both directions mislead: a declared `Option<T>` on a `T` argument tells a client
/// it may omit a parameter the route requires, and a declared `T` on an
/// `Option<T>` argument tells it must send one the route does without.
#[test]
fn a_declared_optionality_the_argument_disagrees_with_fails() {
    let findings = check_source(
        "b2_optionality_the_argument_disagrees_with.rs",
        DISAGREEING_OPTIONALITY,
    );

    assert_eq!(
        render(&findings),
        "b2_optionality_the_argument_disagrees_with.rs:21: get_rows: the annotation declares \
         the parameter \"limit\" as Option<u64>, which utoipa documents as not required, but \
         the handler binds it as u64, so the document and the route disagree about whether a \
         caller may omit it\n\
         b2_optionality_the_argument_disagrees_with.rs:22: get_rows: the annotation declares \
         the parameter \"locate\" as String, which utoipa documents as required, but the \
         handler binds it as Option<String>, so the document and the route disagree about \
         whether a caller may omit it",
        "the conforming parameter in the same annotation is silent, so the rule compares \
         the two optionalities rather than flagging every declared parameter"
    );
}

/// The conforming counterpart: both optionalities agreeing, and a route that
/// declares no parameters at all because `rocket_extras` derives them.
#[test]
fn a_declared_optionality_the_argument_agrees_with_is_accepted() {
    let findings = check_source("b2_conforming.rs", AGREEING_OPTIONALITY);

    assert_eq!(render(&findings), "");
}

/// B3: utoipa takes the declared schema and never compares it to what Rocket
/// parses, so an annotation can advertise a body the route rejects every time.
#[test]
fn a_body_the_route_does_not_parse_fails() {
    let findings = check_source("b3_body_the_route_does_not_parse.rs", UNPARSED_BODY);

    assert_eq!(
        render(&findings),
        "b3_body_the_route_does_not_parse.rs:17: edit_flags: the annotation declares \
         request_body = EditRatingData, but the route binds the body as \
         Json<EditFlagsData>, so the document advertises a schema the route never parses\n\
         b3_body_the_route_does_not_parse.rs:32: import_config: the annotation declares \
         request_body = AppConfig, but the route binds the body as Json<ConfigImport>, so \
         the document advertises a schema the route never parses",
        "a declared body that is not the type the route's `data = \"…\"` binds is a finding \
         in both directions of the mismatch"
    );
}

/// The conforming counterpart, and the three shapes B3 states it does not compare:
/// the same type spelled with a module path, a schema it does not read
/// (`Option<…>`), `request_body = Value` as "any body", and a `Form<…>` binding,
/// which has no schema type an annotation could name.
#[test]
fn a_body_the_route_parses_is_accepted() {
    let findings = check_source("b3_conforming.rs", PARSED_BODY);

    assert_eq!(render(&findings), "");
}

/// B4: a form endpoint takes `multipart/form-data`, and utoipa guesses
/// `application/json` for every named type that is not a primitive — so an
/// annotation that does not name the media type documents a JSON body for a route
/// that parses a multipart upload.
#[test]
fn a_form_body_without_multipart_fails() {
    let findings = check_source("b4_form_body_without_multipart.rs", FORM_WITHOUT_MULTIPART);

    assert_eq!(
        render(&findings),
        "b4_form_body_without_multipart.rs:21: upload: the route binds a form \
         (Form<UploadForm>), so its body is multipart/form-data, but the annotation declares \
         request_body = Value and names no multipart/form-data media type, which utoipa \
         documents as application/json; a generated client would send JSON to a form \
         endpoint\n\
         b4_form_body_without_multipart.rs:35: regenerate_thumbnail: the route binds a form \
         (Form<RegenerateThumbnailForm>), so its body is multipart/form-data, but the \
         annotation declares no request body at all and names no multipart/form-data media \
         type, which utoipa documents as application/json; a generated client would send \
         JSON to a form endpoint",
        "a body that is declared as `Value` and a body that is not declared at all are the \
         same defect; the JSON route in the same fixture is silent, so the rule is about \
         form bindings and not about a missing media type in general"
    );
}

/// The conforming counterpart: both spellings utoipa accepts for naming a media
/// type, and a JSON route that names none — utoipa's default is the right media
/// type for it.
#[test]
fn a_form_body_naming_multipart_is_accepted() {
    let findings = check_source("b4_conforming.rs", FORM_WITH_MULTIPART);

    assert_eq!(render(&findings), "");
}

/// The two forms `params(…)` takes, told apart the way utoipa tells them apart.
///
/// This is the coverage that makes B1 and B2's scope limit a decision rather than
/// an accident: the struct form is *counted*, not skipped, and
/// `the_router_tree_is_clean` pins the count at zero. The day a
/// `#[derive(IntoParams)]` struct reaches an annotation, that pin fails and the
/// resolver either gets built or the limit gets renegotiated — it does not fail
/// open.
#[test]
fn only_the_inline_parameter_form_is_read() {
    let handlers = handlers_in_file("params_forms.rs", PARAMS_FORMS).expect("must parse as Rust");
    let declared: Vec<(String, String)> = handlers[0]
        .annotation
        .params
        .iter()
        .map(|declared| (declared.name.clone(), declared.declared_type.clone()))
        .collect();

    assert_eq!(
        declared,
        vec![("locate".to_owned(), "Option<String>".to_owned())],
        "the inline tuple beside a struct is read, with its declared type"
    );
    assert_eq!(
        handlers[0].annotation.unread_params, 1,
        "the `params(SomeQueryStruct, …)` struct is counted rather than silently skipped: \
         it hides the name, the location and the type behind a type this tool would have \
         to resolve across files"
    );
    assert_eq!(
        handlers[1].annotation.unread_params, 1,
        "a struct mixed with a tuple is one unread entry and one read one"
    );
    assert!(
        !handlers[1].annotation.params[0].declared_optional,
        "`String` is documented as required, which is what B2 compares against the argument"
    );
}

/// A source that declares both forms of `params(…)`, which is the shape the rule's
/// scope limit is about. Written inline rather than pulled in with `include_str!`
/// because it is a reading of the parser rather than a finding to be reported.
const PARAMS_FORMS: &str = r#"
/// Read a timeline page.
#[utoipa::path(
        tag = "timeline",
        params(
            SomeQueryStruct,
            ("locate" = Option<String>, Query, description = "Where to start"),
        ),
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[post("/get/prefetch?<locate>")]
pub fn prefetch(locate: Option<String>) -> AppResult<Json<PrefetchReturn>> {
    let _ = locate;
    Ok(Json(PrefetchReturn::default()))
}

/// Read an asset.
#[utoipa::path(
        tag = "assets",
        params(AssetQuery, ("asset_id" = String, Path)),
        responses(
            (status = 200, description = "Ok"),
        )
    )
]
#[get("/get/get-asset/<asset_id>")]
pub fn get_asset(asset_id: String) -> AppResult<Json<Asset>> {
    let _ = asset_id;
    Ok(Json(Asset::default()))
}
"#;

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

// ── A8 and C3 — what a guard says, and what the document owes it ──────────────
//
// A8 and C3 read the same binding C1 does and the two questions nobody else asks
// about it. A8 asks what the binding is **called**, C3 asks what the annotation
// **declares** about a rejection the route can produce. The fixtures cross-fix
// each other: the A8 fixture documents every rejection status so the findings
// there are A8's alone, and the C3 fixture names every binding after its class so
// the findings there are C3's alone.

/// A8: a guard parameter's name is what a reader scans for when asking "which
/// guard is this", and it is the only handle a finding has on the binding.
///
/// All three shapes are in one fixture: a binding named after a *different*
/// class's binding (`auth` on a `GuardShare`), a binding named after a prefix of
/// its class (`mode` on a `GuardReadOnlyMode`), and the tree's discarded-value
/// spelling on a guard the body **propagates** — where the underscore says the
/// value is thrown away, which is what the `?` is for.
#[test]
fn a_guard_binding_named_after_another_class_fails() {
    let findings = check_source(
        "a8_guard_binding_named_after_another_class.rs",
        MISNAMED_GUARD_BINDINGS,
    );

    assert_eq!(
        render(&findings),
        "a8_guard_binding_named_after_another_class.rs:26: share_as_auth: the guard binding \
         \"auth\" is not named after its guard class: it binds GuardResult<GuardShare>, whose \
         binding is called \"share\", and the name is what a reader of the handler, or of any \
         finding this tool reports about it, uses to say which guard this is\n\
         a8_guard_binding_named_after_another_class.rs:40: rename: the guard binding \"mode\" is \
         not named after its guard class: it binds GuardResult<GuardReadOnlyMode>, whose binding \
         is called \"read_only_mode\", and the name is what a reader of the handler, or of any \
         finding this tool reports about it, uses to say which guard this is\n\
         a8_guard_binding_named_after_another_class.rs:54: discarded_timestamp: the guard binding \
         \"_timestamp\" is not named after its guard class: it binds \
         GuardResult<GuardTimestamp>, whose binding is called \"timestamp\", and the name is what \
         a reader of the handler, or of any finding this tool reports about it, uses to say which \
         guard this is",
        "the rule names the class and the name it expects, and an underscore only excuses a \
         binding whose value the body never propagates"
    );
}

/// A8's second branch: the canonical name is taken and the binding still has to
/// carry the class name as a word.
///
/// The amendment exists because Rocket binds `?<timestamp>` to a handler argument
/// called `timestamp`, so `GuardTimestamp` cannot take its canonical name in any
/// signature in this repository. The match ignores underscores and case, so the
/// fixture carries three accepted spellings — `guard_timestamp`,
/// `timestamp_guard`, and a class name surrounded by other words — and one
/// rejected one.
///
/// **The rejected handler is what keeps this from being an exemption.** Without
/// it the branch would be indistinguishable from "any name is fine when the
/// canonical name is taken", and `taken_name_bindings` would keep reporting four
/// in the scan without anything checking them. It is asserted here rather than
/// only asserted in a comment.
#[test]
fn a_canonical_name_already_taken_still_needs_the_class_name() {
    let findings = check_source("a8_canonical_name_taken.rs", CANONICAL_NAME_TAKEN);

    assert_eq!(
        render(&findings),
        "a8_canonical_name_taken.rs:64: taken_canonical_name_not_carried: the guard binding \
         \"auth\" carries none of its guard class's name: it binds GuardResult<GuardTimestamp>, \
         whose name is \"timestamp\", and another parameter in this signature is already called \
         \"timestamp\", so the binding still has to carry \"timestamp\" as a word — \
         \"timestamp_guard\" and \"guard_timestamp\" both do, and \"auth\" does not",
        "a taken canonical name is not a free pass: a binding that says nothing about the guard \
         it binds is a finding, while all three spellings that carry the class name are silent"
    );
}

/// The conforming counterpart: every class of `GUARD_CLASSES` bound under the name
/// that class expects, in a fallible and in a plain spelling, plus `_`-prefixed
/// plain guards and the mutating shape the tree uses throughout.
///
/// It also pins **A8's scope**: `auth: TimestampGuardModified` is a plain Rocket
/// guard whose name does not begin with `Guard`, so this crate does not classify
/// it and A8 demands no name for it. A rule that needed the class of a type it
/// cannot recognise would have to guess it.
#[test]
fn a_guard_binding_named_after_its_class_is_accepted() {
    let findings = check_source("a8_conforming.rs", GUARD_BINDINGS_AFTER_CLASS);

    assert_eq!(
        render(&findings),
        "",
        "every guard class named after itself must be silent, and a guard type outside \
         GUARD_CLASSES must be out of scope rather than guessed at"
    );
}

/// C3: utoipa publishes exactly the statuses the annotation declares and invents
/// none, so a route that can answer 401 or 405 while the annotation lists neither
/// is an operation whose failure modes a generated client cannot see.
///
/// All three shapes are in one fixture: a credential guard with no 401, the mode
/// guard with no 405 (**what M2 asserted**, now expressed by C3 rather than by a
/// second rule saying the same thing), and a route carrying both guards with only
/// one of the two statuses documented.
#[test]
fn a_guard_rejection_status_the_operation_does_not_document_fails() {
    let findings = check_source(
        "c3_guard_rejection_status_missing.rs",
        MISSING_REJECTION_STATUS,
    );

    assert_eq!(
        render(&findings),
        "c3_guard_rejection_status_missing.rs:19: share_without_401: the route binds \
         GuardShare, which rejects with 401, but the annotation documents (200, 400) and not \
         that status, so the document does not say the operation can answer it\n\
         c3_guard_rejection_status_missing.rs:33: read_only_without_405: the route binds \
         GuardReadOnlyMode, which rejects with 405, but the annotation documents (200, 400, 401) \
         and not that status, so the document does not say the operation can answer it\n\
         c3_guard_rejection_status_missing.rs:51: one_missing_of_two: the route binds \
         GuardReadOnlyMode, which rejects with 405, but the annotation documents (200, 401) and \
         not that status, so the document does not say the operation can answer it",
        "each missing status is one finding; a route carrying two guards and documenting one of \
         the two statuses is reported for the one it leaves out"
    );
}

/// The conforming counterpart: all six credential classes behind one 401, the
/// mode guard behind a 405, both statuses on one route, and a route with no guard
/// at all — for which the rule asks nothing, because nothing can reject it.
#[test]
fn a_documented_guard_rejection_status_is_accepted() {
    let findings = check_source("c3_conforming.rs", DOCUMENTED_REJECTION_STATUS);

    assert_eq!(
        render(&findings),
        "",
        "a guard's rejection status documented once is enough for every binding of its class, and \
         a route with no guard needs none"
    );
}

/// The guard-class table pinned on types rather than on a fixture, the way
/// `only_a_guard_result_carries_an_obligation` pins C1's.
///
/// Two things have to hold and neither is visible from a finding: the `GuardResult`
/// alias is classified by its **payload**, so `GuardResult<GuardShare>` and a bare
/// `GuardShare` are the same class; and the two hash classes are told apart, which
/// a prefix match would get wrong because `GuardHash` is a prefix of
/// `GuardHashOriginal`. A new guard class the table does not name yields `None`
/// rather than a guess, and the tree pin below is what makes the first one a
/// failure.
#[test]
fn guard_classes_resolve_by_payload_and_not_by_prefix() {
    for (ty, binding, status) in GUARD_CLASSES {
        let expected = Some(GuardClass {
            type_name: ty,
            binding,
            rejection_status: status,
        });
        assert_eq!(guard_class(ty), expected, "{ty} as a plain guard");
        assert_eq!(
            guard_class(&format!("GuardResult<{ty}>")),
            expected,
            "{ty} behind the GuardResult alias, which is classified by its payload rather than by \
             the alias"
        );
        assert_eq!(
            guard_class(&format!("crate::router::auth::{ty}")),
            expected,
            "{ty} written with a module path, which is classified on its last segment"
        );
    }

    assert_eq!(
        guard_class("TimestampGuardModified"),
        None,
        "a plain guard whose name does not begin with `Guard` is not a class this tool names, and \
         it is the shape `renew_timestamp_token` binds"
    );
    assert_eq!(
        guard_class("Json<AppConfig>"),
        None,
        "and neither is something that is not a guard at all"
    );
    assert_ne!(
        guard_class("GuardHash"),
        guard_class("GuardHashOriginal"),
        "the two hash classes are told apart, which matching on a prefix would get wrong because \
         `GuardHash` is a prefix of `GuardHashOriginal`"
    );
}

/// The guard inventory the rules were calibrated against: 52 fallible bindings,
/// all consumed as `let _ = ident?;`, and 9 plain `GuardAuth` bindings, none of
/// which the body touches — Rocket runs them and short-circuits on failure.
const GUARD_INVENTORY: (usize, usize) = (52, 9);

/// How many guard bindings A8 and C3 read, and how many of them
/// `GUARD_CLASSES` names.
///
/// The first number is the floor on what A8 reads, the way the annotated-handler
/// count is the floor on the walk: a scan that stopped finding guard bindings
/// would find nothing to name and report the same emptiness as a clean tree. The
/// second is what keeps the class table honest: it equals the first today, and the
/// day a guard class appears that neither A8 nor C3 can classify the two counts
/// part company and this fails — the same treatment `unread_parameters` gets, and
/// for the same reason: a limit that is stated is a decision, and a limit that
/// fails open is not.
const GUARD_CLASS_INVENTORY: (usize, usize) = (61, 61);

/// How many guard bindings A8 judges by its **taken-name** branch, because another
/// parameter of the same signature already holds the canonical name.
///
/// This follows the `unread_parameters` pattern: an exception recorded as a
/// **count** rather than implemented as a mechanism, so it shows up in the scan's
/// own output and a change to it fails a test. All four are the same structural
/// collision — `?<timestamp>` occupying `timestamp` in `get_data.rs:60,191,221`
/// and `get_metadata.rs:38` — and all four are bound as `guard_timestamp`.
///
/// A count that moves means a route changed shape: a `?<timestamp>` went away, so
/// the branch should no longer apply, or a fifth signature collided, so the
/// amendment has a new case to look at. Either way it is a test failure rather
/// than an exception that quietly stopped applying.
const TAKEN_NAME_BINDINGS: usize = 4;

/// The declaration inventory section B was calibrated against: one declared
/// parameter, all of them the inline `("name" = Type, Location, …)` form, and 24
/// declared request bodies.
///
/// The three numbers do three different jobs. The first is a floor on what B1 and
/// B2 read, so a walk that stopped finding `params(…)` fails instead of reporting a
/// clean tree. The second is what keeps the scope limit honest: it is **zero**, and
/// the first `#[derive(IntoParams)]` struct to reach an annotation makes it
/// non-zero, which fails this test rather than narrowing B1 and B2 without a
/// decision. The third is what keeps B3 and B4 honest: a scan that stopped reading
/// `request_body` would find nothing to compare and report the same emptiness as a
/// clean tree.
const DECLARATION_INVENTORY: (usize, usize, usize) = (1, 0, 24);

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
    let guard_bindings: usize = report
        .handlers
        .iter()
        .map(|handler| handler.guard_bindings)
        .sum();
    let classified_guards: usize = report
        .handlers
        .iter()
        .map(|handler| handler.classified_guards)
        .sum();
    let taken_name_bindings: usize = report
        .handlers
        .iter()
        .map(|handler| handler.taken_name_bindings)
        .sum();
    let declared_parameters: usize = report
        .handlers
        .iter()
        .map(|handler| handler.declared_parameters)
        .sum();
    let unread_parameters: usize = report
        .handlers
        .iter()
        .map(|handler| handler.unread_parameters)
        .sum();
    let request_bodies: usize = report
        .handlers
        .iter()
        .map(|handler| handler.request_bodies)
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
        (guard_bindings, classified_guards),
        GUARD_CLASS_INVENTORY,
        "A8 and C3 read less of the tree than they were calibrated against, or they read a guard \
         class `GUARD_CLASSES` does not name. The second number must keep up with the first: a \
         guard class neither rule can classify is out of scope for both, and the day one appears \
         the two counts part company and this fails rather than the rule going quietly blind"
    );
    assert_eq!(
        taken_name_bindings, TAKEN_NAME_BINDINGS,
        "the number of guard bindings whose canonical name is taken by another parameter in the \
         same signature moved. Today all {TAKEN_NAME_BINDINGS} are the `?<timestamp>` collision \
         in get_data.rs and get_metadata.rs, all bound as `guard_timestamp`, which is what A8's \
         taken-name branch is for. A count that moves means a route changed shape — a \
         `?<timestamp>` went away, or a fifth signature collided — so the amendment's scope has \
         to be looked at rather than left applying to a case nobody has read"
    );
    assert_eq!(
        (declared_parameters, unread_parameters, request_bodies),
        DECLARATION_INVENTORY,
        "the tree's declaration inventory moved. `unread_parameters` must stay 0 while \
         B1 and B2 read the inline tuple form only: the first `IntoParams` struct in an \
         annotation needs either a resolver or a renegotiated limit, and a non-zero count \
         fails here rather than narrowing the rules quietly"
    );
    assert_eq!(
        render(&report.findings),
        "",
        "every rule must hold across the tree:\n{}",
        render(&report.findings)
    );
}
