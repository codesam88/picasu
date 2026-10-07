//! Source-level checks on the `#[utoipa::path]` annotations.
//!
//! Three sections of `.plan/openapi-annotation-checks.md` are checked here:
//! **A**, the seven rules about the annotation's own shape; **B**, the four
//! rules about what the annotation declares against what the route already
//! says; and **P**, the four rules comparing `responses(…)` with what the
//! handler can answer — from the return type and body constants (P1), the
//! `FromRequest` impls in the scanned tree (P2), the `ErrorKind` map read from
//! an app-error-map source (P3), and a deliberately over-approximating universe
//! for the declared-side check (P4).
//!
//! Why these belong in the gate. Each of them reads a fact that exists only in
//! source, and each is invisible in the document: the document is generated *from*
//! the annotations, so it cannot disagree with them, and the route table is
//! assembled by a macro. A restated path, a missing `responses(…)`, a tag outside
//! the vocabulary, a handler with no doc comment, a summary wrapped over two lines
//! and a hand-set `operation_id` all produce a document that looks complete while
//! carrying a wrong, missing or unsortable field — and the doc comment case
//! produced 49 operations with no `summary` before the rule existed.
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

use std::path::{Path, PathBuf};

use openapi_sanity::{
    Finding, findings_in_source, handlers_in_file, render, scan_source_root, vocabulary,
};

/// Every rule over one source file, for the fixtures below.
///
/// A fixture that does not parse is a broken test, not a finding, so the error
/// ends the test here rather than being reported as one. The app-error map is
/// one shared input: P3 reads the `ErrorKind` → `http_status` mapping from it
/// the way the gate reads `backend/src/error.rs`.
fn check_source(name: &str, source: &str) -> Vec<Finding> {
    findings_in_source(name, source, APP_ERROR_MAP).unwrap_or_else(|error| panic!("{error}"))
}

/// The P3 input, shaped like `backend/src/error.rs`.
const APP_ERROR_MAP: &str = include_str!("fixtures/app_error_map.rs");

// ── Fixtures ──────────────────────────────────────────────────────────────────

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
const EMPTY_SUMMARY: &str = include_str!("fixtures/openapi_annotations/a5_empty_summary.rs");
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
const QUALIFIED_OPTIONALITY: &str =
    include_str!("fixtures/openapi_annotations/b2_qualified_optionality_agrees.rs");
const AGREEING_OPTIONALITY: &str = include_str!("fixtures/openapi_annotations/b2_conforming.rs");
const UNPARSED_BODY: &str =
    include_str!("fixtures/openapi_annotations/b3_body_the_route_does_not_parse.rs");
const VALUE_SUFFIX_BODY: &str =
    include_str!("fixtures/openapi_annotations/b3_value_suffix_is_not_unconstrained.rs");
const PARSED_BODY: &str = include_str!("fixtures/openapi_annotations/b3_conforming.rs");
const FORM_WITHOUT_MULTIPART: &str =
    include_str!("fixtures/openapi_annotations/b4_form_body_without_multipart.rs");
const FORM_WITH_MULTIPART: &str = include_str!("fixtures/openapi_annotations/b4_conforming.rs");

const STATUS_RETURN_MISSING: &str =
    include_str!("fixtures/openapi_annotations/p1_status_return_missing.rs");
const STATUS_RETURN_CONFORMING: &str =
    include_str!("fixtures/openapi_annotations/p1_status_return_conforming.rs");
const REDIRECT_MISSING: &str = include_str!("fixtures/openapi_annotations/p1_redirect_missing.rs");
const UNREADABLE_STATUS: &str =
    include_str!("fixtures/openapi_annotations/p1_unreadable_status.rs");
const GUARD_MISSING: &str = include_str!("fixtures/openapi_annotations/p2_guard_missing.rs");
const GUARD_CONFORMING: &str = include_str!("fixtures/openapi_annotations/p2_guard_conforming.rs");
const DYNAMIC_GUARD_CONFORMING: &str =
    include_str!("fixtures/openapi_annotations/p2_dynamic_guard_conforming.rs");
const KIND_MISSING: &str = include_str!("fixtures/openapi_annotations/p3_kind_missing.rs");
const KIND_CONFORMING: &str = include_str!("fixtures/openapi_annotations/p3_kind_conforming.rs");
const UNKNOWN_KIND: &str = include_str!("fixtures/openapi_annotations/p3_unknown_kind.rs");
const EXOTIC_STATUS: &str = include_str!("fixtures/openapi_annotations/p4_exotic_status.rs");
const UNIVERSE_CONFORMING: &str =
    include_str!("fixtures/openapi_annotations/p4_universe_conforming.rs");
const ROUTE_400_CONFORMING: &str =
    include_str!("fixtures/openapi_annotations/p4_route_400_conforming.rs");
const IMPOSSIBLE_ROUTE_400: &str =
    include_str!("fixtures/openapi_annotations/p4_impossible_route_400.rs");

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
/// the closed list in `tags.json`.
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
         vocabulary in utils/openapi-sanity/tags.json\n\
         a3_tag_outside_the_vocabulary.rs:14: unknown_tag: the annotation declares the tag \
         \"data\", which is not one of the 10 in utils/openapi-sanity/tags.json (auth, \
         albums, assets, config, index, serving, timeline, upload, pages, internal); a tag \
         outside the vocabulary files the operation outside every section of the reference\n\
         a3_tag_outside_the_vocabulary.rs:24: two_tags: the annotation declares 2 tags \
         (\"assets\", \"timeline\"), but the house rule is exactly one tag per operation, \
         so the reference would file it under all of them",
        "a missing tag, an unknown tag and a repeated tag are three different defects"
    );
}

/// Every tag of the vocabulary is accepted, so A3 is a closed list and not a
/// preference for the tags that happen to be in use.
///
/// The fixture cannot carry all ten without becoming a wall of text, so the
/// second half of this test builds its source from [`vocabulary`] itself: a tag
/// added to the vocabulary is then accepted by construction, and a tag *removed*
/// from it fails, which is the direction that matters. Without it, an eleventh
/// tag could be added to `tags.json` and stay unchecked by the suite.
#[test]
fn every_tag_of_the_vocabulary_is_accepted() {
    let fixture = check_source("a3_conforming.rs", VOCABULARY_TAGS);
    assert_eq!(
        render(&fixture),
        "",
        "one tag of the vocabulary is enough, and `auth` is one of them"
    );

    let source = vocabulary()
        .iter()
        .enumerate()
        .map(|(index, tag)| {
            let tag = tag.name.as_str();
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

/// The tag vocabulary has one source — `tags.json` — and the "Tag conventions"
/// table in the guide is its mirror: this test reads the table back and holds it
/// to the source, name and subject both, so neither can drift alone. The Example
/// column is illustrative and unchecked.
///
/// The table is compared in order: it is the reading order of the section, and
/// `tags.json` is written in the same order.
#[test]
fn the_docs_tag_table_mirrors_the_vocabulary() {
    let docs = include_str!("../../../docs/openapi-generator.md");
    let section = docs
        .split_once("## Tag conventions")
        .expect("the generator guide still has a Tag conventions section")
        .1
        .split("\n## ")
        .next()
        .expect("a split always yields its first part");

    let table: Vec<(&str, &str)> = section
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix('|')?;
            let mut cells = rest.split('|');
            let name = cells.next()?.trim();
            let subject = cells.next()?.trim();
            Some((name.strip_prefix('`')?.strip_suffix('`')?, subject))
        })
        .collect();

    let names: Vec<&str> = vocabulary()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    let subjects: Vec<&str> = vocabulary()
        .iter()
        .map(|entry| entry.description.as_str())
        .collect();

    assert_eq!(
        table.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        names,
        "the table and tags.json must name the same tags in the same order"
    );
    assert_eq!(
        table
            .iter()
            .map(|(_, subject)| *subject)
            .collect::<Vec<_>>(),
        subjects,
        "each Subject cell must be the tags.json description — tags.json is the source \
         of the vocabulary and this table its checked mirror"
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

#[test]
fn an_empty_summary_fails() {
    let findings = check_source("a5_empty_summary.rs", EMPTY_SUMMARY);

    assert_eq!(
        render(&findings),
        "a5_empty_summary.rs:4: empty_summary: the doc comment's first paragraph is empty, \
         so utoipa has no summary text to render",
        "a doc attribute with no text must not satisfy the one-line-summary rule"
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

#[test]
fn qualified_option_types_have_matching_optionality() {
    let findings = check_source("b2_qualified_optionality_agrees.rs", QUALIFIED_OPTIONALITY);

    assert_eq!(
        render(&findings),
        "",
        "a qualified Option type and the bare Option spelling are both optional"
    );
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

#[test]
fn a_custom_type_ending_in_value_is_still_compared() {
    let findings = check_source("b3_value_suffix_is_not_unconstrained.rs", VALUE_SUFFIX_BODY);

    assert_eq!(
        render(&findings),
        "b3_value_suffix_is_not_unconstrained.rs:7: body: the annotation declares \
         request_body = ExpectedValue, but the route binds the body as Json<Actual>, so the \
         document advertises a schema the route never parses"
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

// ── Section P — response statuses ─────────────────────────────────────────────
//
// Section P compares `responses(…)` with what the handler can answer, derived
// from source only: the return type and body constants for the success, the
// `FromRequest` impls in the scanned tree for guards, and the `ErrorKind` map
// for body errors. Required sets are precise (local signals only); the P4
// universe over-approximates so that helper-raised codes never flag.

/// P1: the handler's success is `Status::Accepted`, so the declared 200 both
/// misses 202 and declares something impossible.
#[test]
fn p1_a_success_status_the_handler_never_returns_fails() {
    let findings = check_source("p1_status_return_missing.rs", STATUS_RETURN_MISSING);

    assert_eq!(
        render(&findings),
        "p1_status_return_missing.rs:9: success_status: start_index can answer 202, but \
         responses() does not declare it\np1_status_return_missing.rs:9: declared_status: \
         responses() declares 200, but nothing this handler can answer is 200",
        "the missing success is a P1 finding and the impossible 200 a P4 finding, both at \
         the line they are written on"
    );
}

/// The conforming counterpart: the declared success is the one returned.
#[test]
fn p1_a_returned_success_status_is_accepted() {
    let findings = check_source("p1_status_return_conforming.rs", STATUS_RETURN_CONFORMING);

    assert_eq!(render(&findings), "");
}

/// P1 over the return type rather than a body constant: a `Redirect` answers
/// its constructor's status — `Redirect::to` is 303 See Other, `Redirect::found`
/// is 302. The second handler pins the constructor table's 302 entry.
#[test]
fn p1_a_redirect_declared_as_200_fails() {
    let findings = check_source("p1_redirect_missing.rs", REDIRECT_MISSING);

    assert_eq!(
        render(&findings),
        "p1_redirect_missing.rs:9: success_status: redirect_to_login can answer 303, but \
         responses() does not declare it\np1_redirect_missing.rs:9: declared_status: \
         responses() declares 200, but nothing this handler can answer is 200",
        "Redirect::to is 303, not 302; the Redirect::found handler declaring 302 is \
         silent"
    );
}

/// P1 fail-closed: a `Status` constant this tool cannot map is an explicit
/// finding, and P4 stays quiet while the success is unreadable.
#[test]
fn p1_an_unreadable_success_status_fails() {
    let findings = check_source("p1_unreadable_status.rs", UNREADABLE_STATUS);

    assert_eq!(
        render(&findings),
        "p1_unreadable_status.rs:11: unreadable_status: teapot returns Status, and \
         Status::Teapot is not a Status constant this tool knows",
        "one finding for the unreadable constant; no stale-status finding on top"
    );
}

/// P2: the guard's `FromRequest` impl answers 405 on a locked server, and the
/// annotation does not declare it.
#[test]
fn p2_a_guard_status_missing_from_responses_fails() {
    let findings = check_source("p2_guard_missing.rs", GUARD_MISSING);

    assert_eq!(
        render(&findings),
        "p2_guard_missing.rs:26: guard_status: GuardLocked can answer 405, but responses() \
         does not declare it",
        "the guard's literal outcome status must be declared"
    );
}

/// The conforming counterpart: the guard's status is declared.
#[test]
fn p2_a_declared_guard_status_is_accepted() {
    let findings = check_source("p2_guard_conforming.rs", GUARD_CONFORMING);

    assert_eq!(render(&findings), "");
}

/// P2's scope limit: a computed outcome status makes the guard dynamic, and a
/// dynamic guard requires nothing — the alternative is following helpers this
/// tool does not read. The set of dynamic guards is pinned by the tree test.
#[test]
fn p2_a_dynamic_guard_requires_nothing() {
    let findings = check_source("p2_dynamic_guard_conforming.rs", DYNAMIC_GUARD_CONFORMING);

    assert_eq!(
        render(&findings),
        "",
        "a guard whose status comes from err.http_status() is not a status this tool \
         can require, and must not invent one"
    );
}

/// P3: the body raises `ErrorKind::NotFound`, which `http_status` maps to 404.
#[test]
fn p3_a_body_error_kind_missing_from_responses_fails() {
    let findings = check_source("p3_kind_missing.rs", KIND_MISSING);

    assert_eq!(
        render(&findings),
        "p3_kind_missing.rs:9: body_kind_status: delete_widget raises ErrorKind::NotFound, \
         which answers 404, but responses() does not declare it",
        "the status the app-error map gives the kind must be declared"
    );
}

/// The conforming counterpart: the mapped status is declared.
#[test]
fn p3_a_declared_body_error_kind_is_accepted() {
    let findings = check_source("p3_kind_conforming.rs", KIND_CONFORMING);

    assert_eq!(render(&findings), "");
}

/// P3 fail-closed: a kind the map does not declare is a finding at the
/// occurrence, not a status to guess.
#[test]
fn p3_an_error_kind_the_map_does_not_know_fails() {
    let findings = check_source("p3_unknown_kind.rs", UNKNOWN_KIND);

    assert_eq!(
        render(&findings),
        "p3_unknown_kind.rs:18: unknown_error_kind: delete_widget raises \
         ErrorKind::Databse, which is not a variant of the ErrorKind enum in the \
         app-error map",
        "a typo'd kind is reported where it is written"
    );
}

/// P4: 418 is outside every set the universe is built from.
#[test]
fn p4_an_impossible_declared_status_fails() {
    let findings = check_source("p4_exotic_status.rs", EXOTIC_STATUS);

    assert_eq!(
        render(&findings),
        "p4_exotic_status.rs:11: declared_status: responses() declares 418, but nothing \
         this handler can answer is 418",
        "a declared status no success, guard, body kind or AppError mapping can produce \
         is a finding"
    );
}

/// P4's false-positive guard: 400 with no body literal, raised by a helper the
/// tool does not follow, stays inside the fallible universe.
#[test]
fn p4_a_helper_raised_code_inside_the_universe_is_accepted() {
    let findings = check_source("p4_universe_conforming.rs", UNIVERSE_CONFORMING);

    assert_eq!(
        render(&findings),
        "",
        "the universe over-approximates on purpose: absence of a body literal cannot \
         prove the handler never answers 400"
    );
}

/// P4: a route with a query binding can answer 400 before this non-fallible
/// handler runs — Rocket fails the conversion itself.
#[test]
fn p4_a_query_route_may_declare_400() {
    let findings = check_source("p4_route_400_conforming.rs", ROUTE_400_CONFORMING);

    assert_eq!(render(&findings), "");
}

/// P4: the measured tree case — an infallible handler, a route with no bindings,
/// declaring 400. Nothing can produce it.
#[test]
fn p4_an_undeclarable_status_on_a_bindingless_route_fails() {
    let findings = check_source("p4_impossible_route_400.rs", IMPOSSIBLE_ROUTE_400);

    assert_eq!(
        render(&findings),
        "p4_impossible_route_400.rs:10: declared_status: responses() declares 400, but \
         nothing this handler can answer is 400",
        "no data, no query, no fallibility, no guard, no body kind: 400 is a lie"
    );
}

// ── The real tree ─────────────────────────────────────────────────────────────

/// How many `#[utoipa::path]` annotations the router tree carries today.
///
/// Pinned rather than derived, so a scan that silently stops finding annotations
/// — a broken file walk, a swallowed parse error — fails here instead of
/// reporting a clean tree it never looked at. Update it with the annotation.
const ANNOTATIONS_IN_ROUTER: usize = 63;

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

/// How many of those declared request bodies B3 actually compares against a
/// route binding — the tree's 24 declarations less the one `request_body = Value`
/// (unconstrained, not comparable) and the two form bodies (no nameable schema).
///
/// Without this row B3's comparisons could shrink to nothing — every declaration
/// unreadable, or every binding non-`Json` — and the tree would still report
/// clean. Under a clean tree a comparison is also a match, so 21 compares both
/// that B3 ran and that its results held.
const BODIES_COMPARED: usize = 21;

/// The backend's router tree, resolved from this crate's manifest directory
/// rather than from the working directory a test happens to run in.
fn router_tree() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../backend/src/router")
}

/// The app-error map P3 reads, next to the router tree it describes.
fn app_error_map() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../backend/src/error.rs")
}

/// The guards whose outcome status is computed rather than written as a
/// `Status` constant, pinned so a new one fails here instead of silently
/// requiring nothing.
const DYNAMIC_GUARDS: [&str; 1] = ["GuardShare"];

/// Every guard the scan resolves — each `impl FromRequest for G` under the
/// source root — in the scan's sorted order, pinned so a guard whose impl moves
/// out of the tree fails here instead of silently dropping out of P2 and P4
/// (which both look the guard up by name and skip what they do not find).
const RESOLVED_GUARDS: [&str; 8] = [
    "GuardAuth",
    "GuardHash",
    "GuardHashOriginal",
    "GuardReadOnlyMode",
    "GuardShare",
    "GuardTimestamp",
    "GuardUpload",
    "TimestampGuardModified",
];

/// Every rule over the real router tree, which has to be silent.
///
/// This is the half that proves the rules against the tree rather than against
/// snippets, and the half that pins the scan's coverage so the two cannot drift
/// apart silently. Its name does not name a rule on purpose: a test called after
/// one rule would read as coverage of that rule when it is the run that says
/// whether the whole rule set holds.
#[test]
fn the_router_tree_is_clean() {
    let report = scan_source_root(&router_tree(), &app_error_map())
        .expect("the router tree and the app-error map must be readable");
    assert!(
        report.files_scanned > 1,
        "the scan must see the whole router tree, saw {} file(s)",
        report.files_scanned
    );

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
    let bodies_compared: usize = report
        .handlers
        .iter()
        .map(|handler| handler.bodies_compared)
        .sum();

    assert_eq!(
        report.handlers.len(),
        ANNOTATIONS_IN_ROUTER,
        "the scan must see every annotation in backend/src/router"
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
        bodies_compared, BODIES_COMPARED,
        "B3's comparison count moved. The declaration inventory can hold while every \
         comparison quietly stops happening — unreadable declarations or non-`Json` \
         bindings — so the count of comparisons actually performed is pinned separately"
    );
    assert_eq!(
        report.dynamic_guards, DYNAMIC_GUARDS,
        "the dynamic-guard set moved. A guard whose outcome status is computed requires \
         nothing from P2, so each new one needs an explicit decision here rather than a \
         quiet narrowing"
    );
    assert_eq!(
        report.resolved_guards, RESOLVED_GUARDS,
        "the resolved-guard set moved. P2 and P4 look guards up by name and skip what \
         they do not find, so a `FromRequest` impl leaving the tree must fail here \
         rather than quietly narrowing both rules"
    );
    assert_eq!(
        render(&report.findings),
        "",
        "every rule must hold across the tree:\n{}",
        render(&report.findings)
    );
}
