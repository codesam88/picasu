//! Source-level checks on the `#[utoipa::path]` annotations.
//!
//! Section A of `.plan/openapi-annotation-checks.md` is implemented here: the
//! seven rules about the annotation's own shape. The parameter-agreement (B) and
//! the guard, security (D, M) and mode rules are separate increments.
//!
//! # What this crate holds, and what it must not
//!
//! This tool records **annotation conventions**, and it holds no facts about the
//! backend. Every constant here is a convention someone decided — a tag
//! vocabulary, a set of spellings utoipa also accepts — and every rule reads
//! something written in the `#[utoipa::path]` annotation or in the handler beside
//! it.
//!
//! That boundary is a design decision, not an accident, and the sign it is
//! crossed is recognisable: **if a rule seems to need a route path, a URL prefix,
//! a config value, a feature name, a mount table or a constant from the backend,
//! the rule belongs in the backend or in a just recipe, not here.** A copy of
//! such a fact in this crate is a second place to forget, and the gate built on
//! it is a gate that reports a stale copy rather than the truth. It happened once:
//! A3 briefly carried the backend's contract-exclusion prefixes so the test-only
//! probes could be exempt, and the honest resolution was a vocabulary entry
//! (`internal`) rather than a copy of a path list. The one convention this crate
//! copies rather than read is recorded with a comment saying where the prose is:
//! `TAGS` mirrors a table in `docs/openapi-generator.md`.
//!
//! # Why the rules belong in the gate
//!
//! Why the shape rules belong in the gate. Every one of them is invisible in the
//! generated document, because the document is generated *from* the annotation: a
//! restated path, a missing `responses(…)`, a tag outside the vocabulary, a
//! handler with no doc comment, a summary wrapped over two lines, a hand-set
//! `operation_id` and a hand-set `summary` or `description` each produce a
//! document that looks complete while carrying a wrong, missing or unsortable
//! field. The doc-comment rule is the sharpest case — before it existed, 49 of
//! the 61 published operations had no `summary` at all, and nothing in the
//! document showed that as a defect.
//!
//! # What the scan answers
//!
//! Every assertion reads something the tests decide what to make of, and all of
//! it comes from parsing the files with `syn` rather than comparing two derived
//! views:
//!
//! 1. which functions carry a `#[utoipa::path]` annotation,
//! 2. what the annotation declares ([`Annotation`]),
//! 3. what the handler's doc comment says ([`Located`] lines, and the paragraph
//!    utoipa turns into `summary`).
//!
//! # What section A does not cover
//!
//! The rules assert the *absence* of three spellings utoipa also accepts, each of
//! which restates something the route attribute already says. They match the
//! spellings the plan enumerates, so a residue stays and is review-time rather
//! than gated:
//!
//! - `method(GET)` is the parenthesised verb spelling; A1 rejects only the bare
//!   tokens in `RESTATED_VERBS`.
//! - `tags(["a", "b"])` sets tags in one list; A3 counts `tag = "…"` occurrences
//!   only, and no annotation uses it.
//! - `context_path` sets a base path on the annotation; A1 rejects `path = "…"`.
//!
//! A3's vocabulary is [`TAGS`], which is this repository's copy of the table in
//! `docs/openapi-generator.md` ("Tag conventions"). It is duplicated here rather
//! than read from the document because the tool checks source and the document is
//! a generated review artifact; the two must be changed together.
//!
//! The residue shrank with the work: `trace` moved from unenforced to
//! `RESTATED_VERBS` once it was noticed that utoipa's `HttpMethod` accepts it as
//! a bare token.

use std::fmt;
use std::path::{Path, PathBuf};

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::{Attribute, Expr, ItemFn, Lit, Meta};

/// The closed tag vocabulary of `docs/openapi-generator.md` ("Tag conventions"),
/// which A3 enforces.
///
/// This is the repository's copy of that table. It is duplicated here rather
/// than parsed out of the document because the document is a review artifact
/// generated from these annotations — reading it from here would make the rule
/// check the document with the document. Adding a subject means changing the
/// table in the document and this constant in the same change.
///
/// `internal` is the one entry that names a group the generated reference never
/// renders: the operations outside the published API. Today those are the
/// test-only probes, which `openapi_public` strips from the committed artifact
/// along with the routes under the contract's exclusion prefixes, so a probe has
/// no section to be filed under. The entry says so in the vocabulary rather than
/// leaving those operations untagged, because a rule with an exemption is a rule
/// with a way round it, and a tag in a list is a fact a reader of the source can
/// see. Nothing in this crate decides *which* routes are internal — the backend
/// owns that — so the entry carries a name, not a rule about paths.
pub const TAGS: [&str; 10] = [
    "albums", "assets", "auth", "config", "index", "internal", "pages", "serving", "timeline",
    "upload",
];

/// Bare verb tokens a `#[utoipa::path]` argument must not name, which A1
/// rejects.
///
/// With `rocket_extras` enabled, utoipa reads the verb and the path from the
/// route attribute, so a verb written in the annotation is a second copy of a
/// fact the route already states. Nothing compares the two copies, so a
/// restatement can only rot.
///
/// `trace` is here because it is a Rocket verb and utoipa's `HttpMethod`
/// accepts it, not because a Rocket route ever uses it — the plan's original
/// list of eight omitted it by oversight, and adding a name nothing writes costs
/// nothing while leaving the gap open would let a restatement through.
const RESTATED_VERBS: [&str; 9] = [
    "get", "post", "put", "delete", "head", "options", "patch", "trace", "route",
];

/// One reported problem, rendered as `path:line: identity: what is wrong`.
///
/// The identity comes first inside the text so that a reader scanning the
/// rendered findings sees the handler before the explanation, and the path and
/// line stay machine-usable for a jump-to-source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Path as written by the scan, relative to the source root.
    pub file: String,
    /// 1-based line the finding points at.
    pub line: usize,
    /// `identity: what is wrong`, both halves lowercase.
    pub text: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.file, self.line, self.text)
    }
}

impl Finding {
    /// A finding anchored at a 1-based line of `file`.
    pub fn at(file: &str, line: usize, identity: &str, text: &str) -> Self {
        Self {
            file: file.to_owned(),
            line,
            text: format!("{identity}: {text}"),
        }
    }
}

/// Render findings for an assertion message or a console report, one per line,
/// sorted by location so a diff between two runs shows only what moved.
pub fn render(findings: &[Finding]) -> String {
    let mut sorted: Vec<&Finding> = findings.iter().collect();
    sorted.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    sorted
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

/// A value read from the source together with the 1-based line it was written
/// on, so a finding can point at the token rather than at the handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located<T> {
    /// The value as written.
    pub value: T,
    /// Where it is written, 1-based.
    pub line: usize,
}

impl<T> Located<T> {
    /// A value at a line.
    pub fn new(value: T, line: usize) -> Self {
        Self { value, line }
    }
}

/// What a `#[utoipa::path(…)]` annotation declares, read far enough for the
/// shape rules of section A.
///
/// Only the four things A1–A3 and A6 assert on are kept. Parsed from the
/// attribute's own token stream rather than through `syn::Meta`, so a value this
/// crate does not model — a request body, an extension, a response schema — is
/// skipped over instead of rejected: the annotation grammar belongs to utoipa, and
/// a rule that could not parse a legal annotation would report a tree it never
/// understood.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Annotation {
    /// `path = "…"`, with the line it is written on.
    pub path: Option<Located<String>>,
    /// The first bare verb token, with the line it is written on. One is enough:
    /// the finding names the spelling, and a second one is the same defect.
    pub verb: Option<Located<String>>,
    /// `operation_id = …`, with the line it is written on.
    pub operation_id: Option<Located<String>>,
    /// `summary = "…"`, with the line it is written on. A7 rejects it: utoipa
    /// derives the field from the doc comment otherwise.
    pub summary: Option<Located<String>>,
    /// `description = "…"`, with the line it is written on. A7 rejects it for the
    /// same reason as [`Annotation::summary`].
    pub description: Option<Located<String>>,
    /// Every `tag = "…"`, with the line each is written on.
    pub tags: Vec<Located<String>>,
    /// How many entries `responses(…)` declares, or `None` when the annotation
    /// declares no `responses(…)` at all — a different defect from an empty one.
    pub responses: Option<Located<usize>>,
}

impl Annotation {
    /// Read an annotation's arguments.
    ///
    /// An annotation written as a bare `#[utoipa::path]` has no arguments at all
    /// and yields the default: every rule that requires something to be declared
    /// then reports it, which is the honest reading of an empty annotation.
    pub fn parse(attribute: &Attribute) -> Self {
        let Meta::List(list) = &attribute.meta else {
            return Self::default();
        };
        let mut annotation = Self::default();
        let mut tokens = list.tokens.clone().into_iter();

        while let Some(tree) = tokens.next() {
            let TokenTree::Ident(ident) = &tree else {
                // A stray comma or a value this loop already consumed.
                continue;
            };
            let name = ident.to_string();
            let line = ident.span().start().line;

            match tokens.next() {
                // `name = value`
                Some(TokenTree::Punct(punct)) if punct.as_char() == '=' => {
                    let value = match tokens.next() {
                        Some(TokenTree::Literal(literal)) => literal_text(&literal),
                        // A non-literal value (an operation id built from a const,
                        // say) is still a declaration; what it says does not matter
                        // to a rule that only asks whether it is there.
                        _ => String::new(),
                    };
                    // Read at the top level only. `description` is also the key of
                    // a response entry, and those live inside the `responses(…)`
                    // group, which the arm below never looks into — so A7 cannot
                    // mistake a documented status code for a hand-set description.
                    match name.as_str() {
                        "path" => {
                            annotation
                                .path
                                .get_or_insert_with(|| Located::new(value, line));
                        }
                        "operation_id" => {
                            annotation
                                .operation_id
                                .get_or_insert_with(|| Located::new(value, line));
                        }
                        "summary" => {
                            annotation
                                .summary
                                .get_or_insert_with(|| Located::new(value, line));
                        }
                        "description" => {
                            annotation
                                .description
                                .get_or_insert_with(|| Located::new(value, line));
                        }
                        "tag" => annotation.tags.push(Located::new(value, line)),
                        _ => {}
                    }
                }
                // `name(…)`
                Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => {
                    if name == "responses" {
                        annotation.responses =
                            Some(Located::new(entries_in(&group.stream()), line));
                    }
                }
                // `name` — a bare token, which for a verb is the restatement A1
                // rejects and for anything else is a list A has no rule about.
                _ => {
                    if RESTATED_VERBS.contains(&name.as_str()) {
                        annotation
                            .verb
                            .get_or_insert_with(|| Located::new(name, line));
                    }
                }
            }
        }

        annotation
    }
}

/// The text of a string literal token, without its quotes.
fn literal_text(literal: &proc_macro2::Literal) -> String {
    literal.to_string().trim_matches('"').to_owned()
}

/// How many comma-separated entries a group holds, ignoring empty ones — the
/// shape utoipa parses `responses(…)`, `params(…)` and `extensions(…)` in.
fn entries_in(stream: &TokenStream) -> usize {
    let mut entries = 0;
    let mut open = false;
    for tree in stream.clone() {
        match tree {
            TokenTree::Punct(punct) if punct.as_char() == ',' => {
                if open {
                    entries += 1;
                }
                open = false;
            }
            _ => open = true,
        }
    }
    if open {
        entries += 1;
    }
    entries
}

/// The lines of a handler's doc comment, in source order.
///
/// `///` reaches `syn` as a `#[doc = "…"]` attribute, so this is the doc comment
/// without a second source format to read. The text is trimmed the way a reader
/// sees it, which is also what decides whether a line is a paragraph break.
fn doc_comment(handler: &ItemFn) -> Vec<Located<String>> {
    handler
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("doc"))
        .filter_map(|attribute| {
            let Meta::NameValue(name_value) = &attribute.meta else {
                return None;
            };
            let Expr::Lit(expression) = &name_value.value else {
                return None;
            };
            let Lit::Str(text) = &expression.lit else {
                return None;
            };
            Some(Located::new(
                text.value().trim().to_owned(),
                attribute.span().start().line,
            ))
        })
        .collect()
}

/// A function carrying a `#[utoipa::path]` annotation, with its annotation, doc
/// comment and route arguments resolved.
///
/// No `Debug`: `syn::Block` is only `Debug` under syn's `extra-traits` feature,
/// which this crate does not otherwise need.
#[derive(Clone)]
pub struct AnnotatedHandler {
    /// Path the source was read from, for finding text.
    pub file: String,
    /// The function name — a handler's identity in a finding.
    pub name: String,
    /// What the `#[utoipa::path(…)]` annotation declares.
    pub annotation: Annotation,
    /// The handler's doc comment, line by line. Empty when it carries none.
    pub doc: Vec<Located<String>>,
    /// The line the signature is written on. Private because it exists only to
    /// anchor a finding about the handler as a whole.
    sig_line: usize,
}

impl AnnotatedHandler {
    /// How many lines the doc comment's first paragraph runs, which is the text
    /// utoipa derives `summary` from.
    ///
    /// The paragraph ends at the first blank doc line, so a doc comment with no
    /// blank line at all is one paragraph however long it is.
    pub fn summary_lines(&self) -> usize {
        self.doc
            .iter()
            .take_while(|line| !line.value.is_empty())
            .count()
    }

    /// The line the signature is written on, which is where a reader
    /// looks when the handler has nothing else to point at.
    fn signature_line(&self) -> usize {
        self.sig_line
    }
}

/// A source file that could not be read or parsed.
///
/// A swallowed read or parse error would report a clean tree the scan never
/// looked at, so it is reported instead and ends the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanError {
    /// Path as written by the scan, relative to the source root.
    pub file: String,
    /// What went wrong, prefixed with what was being attempted.
    pub message: String,
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.file, self.message)
    }
}

/// Everything one scan of a source root produced.
///
/// The counts are part of the result rather than of the report the caller
/// prints: they are what pins the scan's coverage, so a walk that silently stops
/// finding annotations fails instead of reporting a tree it never read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeReport {
    /// The root that was scanned, as the caller named it.
    pub source_root: PathBuf,
    /// How many `.rs` files were read.
    pub files_scanned: usize,
    /// Every annotated handler found, in scan order.
    pub handlers: Vec<HandlerSummary>,
    /// Every rule violation, in scan order.
    pub findings: Vec<Finding>,
}

impl TreeReport {
    /// Did the rules pass over everything scanned?
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }

    /// How many annotated handlers the scan saw — the number a floor is checked
    /// against.
    pub fn annotated_handlers(&self) -> usize {
        self.handlers.len()
    }

    /// Check the scan against a floor of annotated handlers.
    ///
    /// The floor is the tool's own blindness check. A walk that stops descending
    /// produces exactly the report a clean tree produces, so a scan below the
    /// floor must not be reported as clean — see [`CoverageShortfall`].
    pub fn check_coverage(&self, expected_at_least: usize) -> Result<(), CoverageShortfall> {
        if self.annotated_handlers() >= expected_at_least {
            return Ok(());
        }
        Err(CoverageShortfall {
            observed: self.annotated_handlers(),
            expected: expected_at_least,
        })
    }
}

/// The scan saw fewer annotated handlers than the floor it was run with.
///
/// This is not a defect in the source tree: it means the scan cannot back the
/// claim it is about to make. A walk that stopped descending, or a path filter
/// that no longer matches, yields the same empty report as a tree with nothing
/// to report — which is the failure this tool exists to prevent, so it is raised
/// as one rather than as a clean run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoverageShortfall {
    /// How many annotated handlers the scan did see.
    pub observed: usize,
    /// The floor the run was given.
    pub expected: usize,
}

impl fmt::Display for CoverageShortfall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the scan found {} annotated handler(s), fewer than the expected minimum of {}: \
             a narrowed file walk is the likely cause, so the tree has not been checked as \
             far as it should have been",
            self.observed, self.expected
        )
    }
}

/// What a handler contributes to a [`TreeReport`]: its source location and name,
/// plus the declaration counts used by the annotation checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerSummary {
    /// Path the source was read from.
    pub file: String,
    /// The function name.
    pub name: String,
}

/// Does this attribute spell `#[utoipa::path(...)]`?
fn is_utoipa_path(attribute: &Attribute) -> bool {
    let segments = &attribute.path().segments;
    segments.len() == 2 && segments[0].ident == "utoipa" && segments[1].ident == "path"
}

/// Collect the `#[utoipa::path]`-annotated functions of a parsed file.
pub fn annotated_handlers(file: &str, parsed: &syn::File) -> Vec<AnnotatedHandler> {
    parsed
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Fn(handler) => Some(handler),
            _ => None,
        })
        .filter(|handler| handler.attrs.iter().any(is_utoipa_path))
        .map(|handler| AnnotatedHandler {
            file: file.to_owned(),
            name: handler.sig.ident.to_string(),
            annotation: Annotation::parse(
                handler
                    .attrs
                    .iter()
                    .find(|attribute| is_utoipa_path(attribute))
                    .expect("the handler was filtered on carrying this attribute"),
            ),
            doc: doc_comment(handler),
            sig_line: handler.sig.span().start().line,
        })
        .collect()
}

/// Parse one source file and return its `#[utoipa::path]`-annotated functions
pub fn handlers_in_file(file: &str, source: &str) -> Result<Vec<AnnotatedHandler>, syn::Error> {
    let parsed: syn::File = syn::parse_file(source)?;
    Ok(annotated_handlers(file, &parsed))
}

/// Name a source file for a finding, relative to the source root.
///
/// Without this a finding would carry the absolute path of whichever worktree
/// the scan ran in, which differs between machines and between a local run and
/// CI. The root itself has no relative name, so it is named as the caller named
/// it — that is the path in an error about the root not being readable.
pub fn relative_name(path: &Path, source_root: &Path) -> String {
    let relative = path.strip_prefix(source_root).unwrap_or(path);
    let named = if relative.as_os_str().is_empty() {
        source_root
    } else {
        relative
    };
    named.to_string_lossy().replace('\\', "/")
}

/// A1 — the annotation restates neither the route's path nor its verb.
///
/// With `rocket_extras` enabled, utoipa derives both from the route attribute,
/// so neither is needed to document the operation. What the annotation adds is a
/// second copy of a fact the route already states, and nothing compares the two
/// copies: the only thing a restatement can do is rot.
fn restated_route(handler: &AnnotatedHandler) -> Vec<Finding> {
    let mut findings = Vec::new();

    if let Some(path) = &handler.annotation.path {
        findings.push(Finding::at(
            &handler.file,
            path.line,
            &handler.name,
            &format!(
                "the annotation declares path = \"{}\", but rocket_extras derives the \
                 path from the route attribute, so a restatement can only be a duplicate \
                 that can rot",
                path.value
            ),
        ));
    }

    if let Some(verb) = &handler.annotation.verb {
        findings.push(Finding::at(
            &handler.file,
            verb.line,
            &handler.name,
            &format!(
                "the annotation names the verb {} as a bare argument, but \
                 rocket_extras derives the verb from the route attribute, so a \
                 restatement can only be a duplicate that can rot",
                verb.value
            ),
        ));
    }

    findings
}

/// A2 — `responses(…)` is present and declares at least one entry.
///
/// utoipa invents no response, so an annotation without them produces an
/// operation that documents nothing it can answer. An empty `responses()` is the
/// same defect spelled differently and is reported separately, because it reads
/// as though responses had been considered.
fn responses_declared(handler: &AnnotatedHandler) -> Vec<Finding> {
    match &handler.annotation.responses {
        Some(entries) if entries.value > 0 => Vec::new(),
        Some(entries) => vec![Finding::at(
            &handler.file,
            entries.line,
            &handler.name,
            "the annotation declares responses() with no entry, and utoipa invents no \
             response, so the operation documents nothing it can answer",
        )],
        None => vec![Finding::at(
            &handler.file,
            handler.signature_line(),
            &handler.name,
            "the annotation declares no responses, and utoipa invents no response, so \
             the operation documents nothing it can answer",
        )],
    }
}

/// A3 — exactly one `tag`, and it is one of [`TAGS`].
///
/// The tag is what the generated reference groups operations by, so a missing or
/// misspelled one files the operation nowhere a reader looks. The vocabulary is
/// the table in `docs/openapi-generator.md` ("Tag conventions"), which this
/// repository copies into [`TAGS`]; the tool owns the list because a document
/// linter is not adopted.
///
/// The rule is absolute: every annotated operation declares one tag, with no
/// exemptions for routes the published document drops. `internal` is how such an
/// operation says so — a vocabulary entry, not a hole in the rule — so the
/// surface that is stripped from the committed artifact is still declared in the
/// same shape as everything else, and a reader of the tree can see it in the
/// place a reader looks for it.
fn one_vocabulary_tag(handler: &AnnotatedHandler) -> Vec<Finding> {
    match handler.annotation.tags.as_slice() {
        [] => vec![Finding::at(
            &handler.file,
            handler.signature_line(),
            &handler.name,
            "the annotation declares no tag, so the operation is filed nowhere in the \
             generated reference; take one from the vocabulary in docs/openapi-generator.md \
             \"Tag conventions\"",
        )],
        [tag] if TAGS.contains(&tag.value.as_str()) => Vec::new(),
        [tag] => vec![Finding::at(
            &handler.file,
            tag.line,
            &handler.name,
            &format!(
                "the annotation declares the tag \"{}\", which is not one of the {} in \
                 docs/openapi-generator.md \"Tag conventions\" ({}); a tag outside the \
                 vocabulary files the operation outside every section of the reference",
                tag.value,
                TAGS.len(),
                TAGS.join(", ")
            ),
        )],
        tags => vec![Finding::at(
            &handler.file,
            tags[0].line,
            &handler.name,
            &format!(
                "the annotation declares {} tags ({}), but the house rule is exactly one \
                 tag per operation, so the reference would file it under all of them",
                tags.len(),
                tags.iter()
                    .map(|tag| format!("\"{}\"", tag.value))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )],
    }
}

/// A4 — the handler carries a doc comment.
///
/// utoipa derives `summary` and `description` from it and the reference renders
/// both, so a handler without one is an operation with no text in the generated
/// documentation — invisible in the document, and invisible in review of the
/// document, because the document looks complete.
fn doc_comment_present(handler: &AnnotatedHandler) -> Vec<Finding> {
    if !handler.doc.is_empty() {
        return Vec::new();
    }
    vec![Finding::at(
        &handler.file,
        handler.signature_line(),
        &handler.name,
        "the handler carries no doc comment, and summary and description are derived \
         from it, so the operation reaches the generated reference with neither",
    )]
}

/// A5 — the doc comment's first paragraph is one line.
///
/// That paragraph is the operation's `summary`, and `widdershins` renders the
/// summary as the reference's heading. A paragraph of more than one line puts a
/// newline inside a markdown heading, which splits it. This was measured on this
/// repository's own document, not adopted as a style opinion.
fn one_line_summary(handler: &AnnotatedHandler) -> Vec<Finding> {
    let Some(first_line) = handler.doc.first() else {
        return Vec::new();
    };
    let summary_lines = handler.summary_lines();
    if summary_lines == 1 {
        return Vec::new();
    }
    if summary_lines == 0 {
        return vec![Finding::at(
            &handler.file,
            first_line.line,
            &handler.name,
            "the doc comment's first paragraph is empty, so utoipa has no summary text to render",
        )];
    }
    let second_line = &handler.doc[1];
    // Anchored at the second line of the paragraph: that is where the heading
    // stops being a heading.
    vec![Finding::at(
        &handler.file,
        second_line.line,
        &handler.name,
        &format!(
            "the doc comment's first paragraph must be one line, and it is the \
             operation's summary, which the reference renders as a heading; a heading \
             must be one line, and this paragraph is {} line(s)",
            handler.summary_lines()
        ),
    )]
}

/// A6 — the annotation sets no `operation_id`.
///
/// utoipa derives it from the function name, and every other name in the document
/// is either derived the same way or compared by `openapi-routes-match`. A
/// hand-set `operation_id` is the one name nothing compares: it can be changed
/// without changing a route, and the parity check will not notice.
fn no_hand_set_operation_id(handler: &AnnotatedHandler) -> Vec<Finding> {
    handler
        .annotation
        .operation_id
        .as_ref()
        .map_or_else(Vec::new, |id| {
            vec![Finding::at(
                &handler.file,
                id.line,
                &handler.name,
                &format!(
                    "the annotation sets operation_id = \"{}\", which utoipa otherwise derives \
                 from the function name; a hand-set one is the only name in the document \
                 that nothing compares",
                    id.value
                ),
            )]
        })
}

/// A7 — the annotation sets neither `summary` nor `description`.
///
/// utoipa derives both from the doc comment: `summary` from its first paragraph
/// and `description` from the rest. A hand-set one is therefore the same prose
/// twice, and the two copies are compared by nothing — the same argument as A6,
/// where the derived name is the only one a consumer can predict. A7 exists
/// because A5's premise is false without it: while an annotation may set
/// `summary`, "the first paragraph *is* the summary" does not hold, and A5 would
/// be reporting a defect the document does not have.
///
/// A7 does not extend to a per-response `description`, which is how a status
/// code's text is written and is not derived from anything.
fn no_hand_set_prose(handler: &AnnotatedHandler) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (key, declared) in [
        ("summary", &handler.annotation.summary),
        ("description", &handler.annotation.description),
    ] {
        let Some(declared) = declared else {
            continue;
        };
        findings.push(Finding::at(
            &handler.file,
            declared.line,
            &handler.name,
            &format!(
                "the annotation sets {key} = \"{}\", which utoipa otherwise derives from the \
                 doc comment; a hand-set one is prose written twice beside itself, and \
                 nothing compares the two copies",
                declared.value
            ),
        ));
    }
    findings
}

/// A1 to A7 of section A, then C1: every rule over one handler, in a fixed
/// order so a report reads the same way twice.
///
/// C1b contributes nothing here for the reason given on
/// [`findings_in_source`].
fn rules_over(handler: &AnnotatedHandler) -> Vec<Finding> {
    [
        restated_route(handler),
        responses_declared(handler),
        one_vocabulary_tag(handler),
        doc_comment_present(handler),
        one_line_summary(handler),
        no_hand_set_operation_id(handler),
        no_hand_set_prose(handler),
    ]
    .concat()
}

/// Every annotation rule over one source file: sections A and B.
pub fn findings_in_source(name: &str, source: &str) -> Result<Vec<Finding>, ScanError> {
    handlers_in_file(name, source)
        .map_err(|error| ScanError {
            file: name.to_owned(),
            message: format!("must parse as Rust: {error}"),
        })
        .map(|handlers| handlers.iter().flat_map(rules_over).collect())
}

/// Read every `.rs` file under `source_root`, recursively, as `(name, source)`
/// pairs sorted by name so a scan of the same tree is reproducible.
fn rust_sources(source_root: &Path) -> Result<Vec<(String, String)>, ScanError> {
    fn walk(
        directory: &Path,
        source_root: &Path,
        sources: &mut Vec<(String, String)>,
    ) -> Result<(), ScanError> {
        let entries = std::fs::read_dir(directory).map_err(|error| ScanError {
            file: relative_name(directory, source_root),
            message: format!("must be readable: {error}"),
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| ScanError {
                file: relative_name(directory, source_root),
                message: format!("must be readable: {error}"),
            })?;
            let path = entry.path();
            if path.is_dir() {
                walk(&path, source_root, sources)?;
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let name = relative_name(&path, source_root);
                let source = std::fs::read_to_string(&path).map_err(|error| ScanError {
                    file: name.clone(),
                    message: format!("must be readable: {error}"),
                })?;
                sources.push((name, source));
            }
        }
        Ok(())
    }

    let mut sources = Vec::new();
    walk(source_root, source_root, &mut sources)?;
    sources.sort();
    Ok(sources)
}

/// Every rule over every `.rs` file under `source_root`.
///
/// The findings, the file count and one [`HandlerSummary`] per annotated handler
/// all come from the same walk, so a caller can assert on coverage as well as on
/// findings without walking the tree a second time.
pub fn scan_source_root(source_root: &Path) -> Result<TreeReport, ScanError> {
    let sources = rust_sources(source_root)?;
    let mut report = TreeReport {
        source_root: source_root.to_owned(),
        files_scanned: sources.len(),
        handlers: Vec::new(),
        findings: Vec::new(),
    };

    for (name, source) in &sources {
        let handlers = handlers_in_file(name, source).map_err(|error| ScanError {
            file: name.clone(),
            message: format!("must parse as Rust: {error}"),
        })?;
        for handler in &handlers {
            report.findings.extend(rules_over(handler));
            report.handlers.push(HandlerSummary {
                file: handler.file.clone(),
                name: handler.name.clone(),
            });
        }
    }

    Ok(report)
}
