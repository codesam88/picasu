//! Source-level checks on the `#[utoipa::path]` annotations.
//!
//! Two sections of `.plan/openapi-annotation-checks.md` are checked here: **A**,
//! the seven rules about the annotation's own shape, and **B**, the four rules
//! about what the annotation declares against what the route already says.
//! Security rules are deliberately deferred until runtime security tests exist.
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
    Finding, TAGS, findings_in_source, handlers_in_file, render, scan_source_root,
};

/// Every rule over one source file, for the fixtures below.
///
/// A fixture that does not parse is a broken test, not a finding, so the error
/// ends the test here rather than being reported as one.
fn check_source(name: &str, source: &str) -> Vec<Finding> {
    findings_in_source(name, source).unwrap_or_else(|error| panic!("{error}"))
}

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
