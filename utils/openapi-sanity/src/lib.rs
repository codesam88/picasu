//! Source-level checks on the `#[utoipa::path]` annotations.
//!
//! Two sections of `.plan/openapi-annotation-checks.md` are implemented here:
//! **section A**, the seven rules about the annotation's own shape, and **section
//! B**, the four rules about what the annotation declares against what the route
//! already says. Guard and security semantics are deliberately deferred to a
//! separate increment with runtime tests.
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
//! Why the parameter rules belong in the gate. `rocket_extras` reads the route
//! attribute, so the *route* wins: it supplies the path, the verb, and a derived
//! parameter and request body for every argument the handler binds. What an
//! annotation declares on top of that is merged in, and utoipa compares none of
//! it against what it just derived — so an annotation can advertise a parameter
//! no route reads, an optionality the route does not have, a body type the route
//! never parses, or a JSON media type for a form endpoint. Each of those is a
//! document that is wrong in a way no generated client can detect before the
//! call fails, and none of them shows up as anything in the document itself.
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
//!    utoipa turns into `summary`),
//! 4. what the route attribute binds ([`Route`]: its path segments, its query
//!    names and its `data = "…"` body argument),
//! 5. what each handler argument's type is ([`HandlerArgument`]), which is where
//!    an `Option`, a `Json<…>`, a `Data<…>` and a `Form<…>` are told apart.
//!
//! # What section B does not cover, and why
//!
//! B1 and B2 read a **declared parameter's** name, location and type. utoipa
//! accepts two spellings for a parameter in `params(…)`, and only one is read:
//!
//! - The **inline tuple** — `("name" = Type, Query, description = "…")` — is
//!   read directly, and is the only form this repository uses.
//! - The **struct** — `params(SomeQueryStruct)`, or a struct mixed with tuples —
//!   hides the name, the location and the type behind a type the tool would have
//!   to resolve across files (find the `#[derive(IntoParams)]`, read its fields,
//!   apply its `#[param(…)]` overrides). No type in this repository derives
//!   `IntoParams`, so a resolver would ship untested against real code, and the
//!   entries are **counted rather than skipped**: [`Annotation::unread_params`]
//!   carries how many a scan could not read, `the_router_tree_is_clean` pins
//!   that count, and the first struct form to appear fails the pin rather than
//!   silently narrowing the rule.
//!
//! Two further limits are properties of utoipa's grammar rather than choices:
//!
//! - **A declared parameter has no `required` key.** utoipa 5.5's parameter
//!   feature list rejects it as an unknown attribute, and derives the documented
//!   `required` from the declared type alone (`!is_option`). So B2 compares the
//!   declared type's optionality with the handler argument's, which is what the
//!   document's `required` ends up saying either way.
//! - **A `Form<…>` binding has no schema type an annotation could name.** The
//!   payload carries `TempFile<'r>` and a lifetime, so B3 has nothing to compare
//!   and does not compare it. B4 covers what it *can* check — that the media type
//!   is `multipart/form-data` rather than utoipa's default.
//!
//! B1 also skips a declared parameter whose location is neither `Path` nor
//! `Query`: a header or cookie is not named anywhere in a Rocket route
//! attribute, so there is no route binding for it to disagree with.
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
use syn::{Attribute, Expr, FnArg, GenericArgument, ItemFn, Lit, Meta, Pat, PathArguments, Type};

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

/// The Rocket verbs a route attribute can be written with, which is how a route
/// attribute is told from any other attribute on a handler.
///
/// A separate list from [`RESTATED_VERBS`] on purpose: that one is what utoipa's
/// `HttpMethod` accepts as a bare argument inside an annotation, and it holds
/// `trace` and `route`, which a Rocket route attribute is never written with.
/// This one is what `#[get]` / `#[post]` / … is.
const ROUTE_VERBS: [&str; 7] = ["get", "post", "put", "delete", "patch", "head", "options"];

/// Where a declared parameter is read from — utoipa's `ParameterIn`.
///
/// Only [`ParameterLocation::Path`] and [`ParameterLocation::Query`] are
/// checked, because only those two are named anywhere in a Rocket route
/// attribute. A header or cookie has no spelling in the route, so B1 has
/// nothing to compare it against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterLocation {
    /// `<segment>` in the route's path.
    Path,
    /// `?<name>` in the route's query part.
    Query,
    /// `<header>` in the request.
    Header,
    /// `<cookie>` in the request.
    Cookie,
}

/// One parameter an annotation declares inline: `("name" = Type, Query, …)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredParameter {
    /// The declared name, which is the key the document publishes.
    pub name: String,
    /// Where the annotation says it is read from.
    pub location: ParameterLocation,
    /// The declared type as written (`Option<bool>`, `String`).
    pub declared_type: String,
    /// Whether the declared type is an `Option<…>`, which is — for lack of any
    /// `required` key in utoipa's grammar — exactly what makes the document call
    /// the parameter optional.
    pub declared_optional: bool,
    /// The line the parameter is written on.
    pub line: usize,
}

/// What a `request_body` declaration puts in the document's schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclaredBody {
    /// `request_body = Type` — the named type, which is what B3 compares with
    /// the route's binding.
    Type(String),
    /// A declaration whose schema this tool does not read: `inline(…)`, a
    /// `[T]` array, `Option<…>`, or the `request_body(content = …)` group form.
    Unreadable,
}

impl DeclaredBody {
    /// The declared type as written, if the declaration names one.
    pub fn type_name(&self) -> Option<&str> {
        match self {
            Self::Type(name) => Some(name),
            Self::Unreadable => None,
        }
    }
}

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
/// shape rules of section A and the declaration rules of section B.
///
/// Only what a rule asserts on is kept. Parsed from the attribute's own token
/// stream rather than through `syn::Meta`, so a value this crate does not model —
/// an extension, a response schema — is skipped over instead of rejected: the
/// annotation grammar belongs to utoipa, and a rule that could not parse a legal
/// annotation would report a tree it never understood.
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
    /// Every parameter `params(…)` declares in the inline tuple form. The struct
    /// form is not read; see [`Annotation::unread_params`].
    pub params: Vec<DeclaredParameter>,
    /// How many `params(…)` entries are in a form the rules cannot read. Counted
    /// rather than skipped, so that the first one to appear fails a coverage pin
    /// instead of quietly narrowing B1 and B2.
    pub unread_params: usize,
    /// `request_body`, with the line it is written on, or `None` when the
    /// annotation declares none.
    pub request_body: Option<Located<DeclaredBody>>,
    /// The media type the `request_body` names explicitly, either
    /// `request_body(content_type = "…")` or a `request_body(content(… = …))`
    /// group. `None` means the annotation names none and utoipa guesses one.
    pub body_content_type: Option<Located<String>>,
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
        let mut cursor = list.tokens.clone().into_iter();
        let tokens = &mut cursor;

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
                    // The value runs to the next top-level comma, which is more
                    // than one token for a path (`serde_json::Value`) or a
                    // generic argument list.
                    let mut rest = tokens.clone();
                    let mut value = String::new();
                    let mut value_is_group = false;
                    for tree in rest.by_ref() {
                        match &tree {
                            TokenTree::Punct(punct) if punct.as_char() == ',' => break,
                            TokenTree::Group(group) => {
                                value_is_group = matches!(
                                    group.delimiter(),
                                    Delimiter::Parenthesis | Delimiter::Bracket
                                );
                                value.push_str(&group.to_string());
                            }
                            tree => match tree {
                                // A string literal is read as its text, not as
                                // its source spelling, so a tag is `albums`
                                // rather than `"albums"`.
                                TokenTree::Literal(literal) => {
                                    value.push_str(&literal_text(literal))
                                }
                                tree => value.push_str(&tree.to_string()),
                            },
                        }
                    }
                    *tokens = rest;
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
                        // `request_body = Type` names a schema; `inline(…)`, a
                        // `[T]` array and `Option<…>` are declarations whose
                        // schema this crate does not read.
                        "request_body" => {
                            let body = if value_is_group || head_segment(&value) == "Option" {
                                DeclaredBody::Unreadable
                            } else {
                                // A type path is written `serde_json :: Value`;
                                // the spaces are the token stream's, not the
                                // reader's.
                                DeclaredBody::Type(value.replace(' ', ""))
                            };
                            annotation
                                .request_body
                                .get_or_insert_with(|| Located::new(body, line));
                        }
                        _ => {}
                    }
                }
                // `name(…)`
                Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => {
                    match name.as_str() {
                        "responses" => {
                            annotation.responses =
                                Some(Located::new(entries_in(&group.stream()), line));
                        }
                        "params" => read_params(&group.stream(), &mut annotation),
                        "request_body" => {
                            read_request_body_group(&group.stream(), line, &mut annotation)
                        }
                        _ => {}
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

/// Read `params(…)`: every inline `("name" = Type, Location, …)` entry becomes a
/// [`DeclaredParameter`], and every entry in any other form is counted as
/// unread.
///
/// The two forms are told apart the way utoipa tells them apart — an entry that
/// is parenthesised is a tuple, anything else is a type to resolve — which is why
/// a struct form is counted rather than guessed at. See [`Annotation::unread_params`]
/// for why that count is a coverage pin and not a silent skip.
fn read_params(stream: &TokenStream, annotation: &mut Annotation) {
    for entry in split_entries(stream) {
        let mut trees = entry.into_iter();
        let Some(TokenTree::Group(tuple)) = trees.next() else {
            annotation.unread_params += 1;
            continue;
        };
        let line = tuple.span().start().line;

        // `"name" = Type` is the first entry; the location is the next one, and
        // whatever follows that is a feature (`description`, `example`, …).
        let fields = split_entries(&tuple.stream());
        let mut head = fields
            .first()
            .map_or_else(TokenStream::new, Clone::clone)
            .into_iter();
        let Some(TokenTree::Literal(name)) = head.next() else {
            annotation.unread_params += 1;
            continue;
        };
        let declared = head
            .filter(|tree| !matches!(tree, TokenTree::Punct(punct) if punct.as_char() == '='))
            .map(|tree| tree.to_string())
            .collect::<Vec<_>>()
            .join("")
            .replace(' ', "");
        let location = fields
            .get(1)
            .map(|field| field.to_string())
            .and_then(|field| {
                parameter_location(field.split_whitespace().next().unwrap_or_default())
            });
        let Some(location) = location else {
            annotation.unread_params += 1;
            continue;
        };
        annotation.params.push(DeclaredParameter {
            name: literal_text(&name),
            location,
            declared_optional: type_is_optional(&declared),
            declared_type: declared,
            line,
        });
    }
}

/// utoipa's `ParameterIn` as the token it is written with. `None` for anything
/// else, which puts the entry outside what B1 and B2 can read.
fn parameter_location(token: &str) -> Option<ParameterLocation> {
    match token {
        "Path" => Some(ParameterLocation::Path),
        "Query" => Some(ParameterLocation::Query),
        "Header" => Some(ParameterLocation::Header),
        "Cookie" => Some(ParameterLocation::Cookie),
        _ => None,
    }
}

/// Read `request_body(…)`: the media type it names explicitly, and a body whose
/// schema is a `content = …` value this crate does not read.
///
/// `content_type = "…"` names the media type on its own; the group form,
/// `content("mime" = Schema)`, names it as the key of each media-type entry. Both
/// spellings are read, because both say the same thing and utoipa rejects the
/// combination of the two.
fn read_request_body_group(stream: &TokenStream, line: usize, annotation: &mut Annotation) {
    annotation
        .request_body
        .get_or_insert_with(|| Located::new(DeclaredBody::Unreadable, line));

    for entry in split_entries(stream) {
        let mut trees = entry.into_iter();
        let Some(TokenTree::Ident(key)) = trees.next() else {
            continue;
        };
        let key_line = key.span().start().line;
        // `content_type = "mime"` names the media type on its own; the group form,
        // `content(Schema = "mime")`, names it inside the `content` argument.
        let media_type = match key.to_string().as_str() {
            "content_type" => trees.find_map(|tree| match tree {
                TokenTree::Literal(literal) => Some(literal_text(&literal)),
                _ => None,
            }),
            "content" => trees.find_map(|tree| match tree {
                TokenTree::Group(group) => first_literal(&group.stream()),
                _ => None,
            }),
            _ => None,
        };
        if let Some(media_type) = media_type {
            annotation
                .body_content_type
                .get_or_insert_with(|| Located::new(media_type, key_line));
        }
    }
}

/// The text of the first string literal anywhere in these tokens, descending into
/// groups.
///
/// utoipa's group form nests its media type inside a per-entry parenthesis —
/// `content((Object = "multipart/form-data"))` — so the depth is not fixed and
/// the only honest reading is the first literal at whatever depth it sits.
fn first_literal(stream: &TokenStream) -> Option<String> {
    stream.clone().into_iter().find_map(|tree| match tree {
        TokenTree::Literal(literal) => Some(literal_text(&literal)),
        TokenTree::Group(group) => first_literal(&group.stream()),
        _ => None,
    })
}

/// Split a group into its comma-separated entries, the way utoipa parses
/// `params(…)`, `responses(…)` and `extensions(…)`.
///
/// Angle brackets nest, so a comma inside a generic argument list — `("x" =
/// HashMap<String, u32>, Query)` — does not end the entry. Nothing else needs
/// tracking: every other delimiter reaches `syn` as a group, and a comma inside
/// one is already part of that group.
fn split_entries(stream: &TokenStream) -> Vec<TokenStream> {
    let mut entries = Vec::new();
    let mut current = TokenStream::new();
    let mut depth: usize = 0;
    for tree in stream.clone() {
        if let TokenTree::Punct(punct) = &tree {
            match punct.as_char() {
                '<' => depth += 1,
                '>' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    if !current.is_empty() {
                        entries.push(std::mem::take(&mut current));
                    }
                    continue;
                }
                _ => {}
            }
        }
        current.extend(Some(tree));
    }
    if !current.is_empty() {
        entries.push(current);
    }
    entries
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

/// Is a type as written an `Option<…>`?
///
/// The test is on the last path segment rather than on a prefix, so both
/// `Option<T>` and `std::option::Option<T>` are optional, but a type merely named
/// `OptionalThing` is not.
fn type_is_optional(text: &str) -> bool {
    head_segment(text).rsplit("::").next() == Some("Option")
}

/// The outermost type name of a type as written: `Json<CreateAlbum>` is `Json`,
/// `serde_json::Value` is `serde_json::Value`.
fn head_segment(text: &str) -> &str {
    text.split('<').next().unwrap_or(text).trim()
}

/// What a handler argument's type says the request body is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyBinding {
    /// `Json<T>` or `Data<T>`, optionally through an `Option` — a JSON body the
    /// route parses into `T`.
    Json(String),
    /// `Form<T>`, optionally through an `Option` or a `Result<Form<T>, Errors>` —
    /// a `multipart/form-data` body.
    Form(String),
    /// Not a body binding: a guard, a plain query argument, or a type these rules
    /// do not read.
    None,
}

/// Read the body a handler argument's type binds.
///
/// `Option<…>` and a `Result<…>` are peeled because both appear around a body
/// guard in Rocket — `Option<Json<T>>` for an optional body, and
/// `Result<Form<T>, Errors<'_>>` for a form Rocket may fail to parse — and the
/// guard underneath is what says what the body is.
pub fn body_binding(type_as_written: &str) -> BodyBinding {
    let text = type_as_written.replace(' ', "");
    let head = head_segment(&text);
    let payload = text
        .split_once('<')
        .map(|(_, rest)| rest.trim_end_matches('>'))
        .unwrap_or_default();

    match head {
        "Option" | "Result" => body_binding(payload),
        "Json" | "Data" => BodyBinding::Json(payload.to_owned()),
        "Form" => BodyBinding::Form(payload.to_owned()),
        _ => BodyBinding::None,
    }
}

/// One handler argument of an annotated handler, with the type as written.
///
/// Every typed argument is kept, not only the guards: B2 compares a declared
/// parameter against the argument of the same name, and that argument need not be
/// a guard.
#[derive(Debug, Clone)]
pub struct HandlerArgument {
    /// The parameter name.
    pub ident: String,
    /// The type as written (`Option<bool>`, `Json<CreateDirAlbumData>`).
    pub ty: String,
    /// Where the parameter is written in the signature.
    pub span: proc_macro2::Span,
}

impl HandlerArgument {
    /// Is the argument an `Option<…>`?
    pub fn is_optional(&self) -> bool {
        type_is_optional(&self.ty)
    }

    /// What request body this argument binds, if any.
    pub fn body(&self) -> BodyBinding {
        body_binding(&self.ty)
    }
}

/// What a handler's route attribute binds: the path it serves, the names its
/// path and query parts bind, and the argument its body comes from.
///
/// Read from the same attribute utoipa reads it from, which is the whole point:
/// the route is what the annotation is checked against, so it has to be the same
/// route rather than a re-derivation of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Route {
    /// The route path as written, e.g. `/albums/<id>?<locate>`.
    pub path: Option<Located<String>>,
    /// The `<segment>` names of the path part, a trailing `..` removed.
    pub path_segments: Vec<String>,
    /// The `?<name>` names of the query part, a trailing `..` removed.
    pub query: Vec<String>,
    /// The argument `data = "<name>"` binds the body to.
    pub data: Option<Located<String>>,
}

/// Is this attribute a Rocket route attribute — `#[get("…")]`, `#[post("…", …)]`?
fn is_route_attribute(attribute: &Attribute) -> bool {
    attribute
        .path()
        .get_ident()
        .is_some_and(|ident| ROUTE_VERBS.contains(&ident.to_string().as_str()))
}

/// Read the route attribute of a handler, or `None` when it carries none.
fn route_attribute(handler: &ItemFn) -> Option<Route> {
    let attribute = handler
        .attrs
        .iter()
        .find(|attribute| is_route_attribute(attribute))?;
    let Meta::List(list) = &attribute.meta else {
        return None;
    };
    let mut trees = list.tokens.clone().into_iter();
    let line = list.span().start().line;

    // `#[get("/path", data = "<body>")]`: the path is the first literal, and
    // every other argument is a `key = value` pair.
    let Some(TokenTree::Literal(path)) = trees.next() else {
        return None;
    };
    let path = literal_text(&path);
    let (segments, query) = route_names(&path);

    let mut data = None;
    while let Some(tree) = trees.next() {
        // Commas separate the arguments; a route attribute writes them after the
        // path, and the path may be followed by several of them.
        let TokenTree::Ident(key) = &tree else {
            continue;
        };
        let key_line = key.span().start().line;
        if key != "data" {
            continue;
        }
        // `data = "<argument>"`: the binding is a string literal, and the angle
        // brackets around the name are part of the literal, not tokens.
        if matches!(trees.next(), Some(TokenTree::Punct(punct)) if punct.as_char() == '=')
            && let Some(TokenTree::Literal(name)) = trees.next()
        {
            let name = literal_text(&name);
            data = Some(Located::new(
                name.trim_matches(['<', '>']).to_owned(),
                key_line,
            ));
        }
    }

    Some(Route {
        path: Some(Located::new(path, line)),
        path_segments: segments,
        query,
        data,
    })
}

/// The `<segment>` names of a route path and the `?<name>` names of its query.
///
/// A partial-segment name is written `<name..>`; the `..` is Rocket's marker and
/// not part of the name the handler binds, so it is dropped. Anything after a
/// `?` is the query part, and a literal beside a dynamic name (`?<a>foo`) still
/// binds `a`.
fn route_names(path: &str) -> (Vec<String>, Vec<String>) {
    let (head, tail) = path.split_once('?').unwrap_or((path, ""));
    let segments = dynamic_names(head.split('/'));
    let query = dynamic_names(tail.split('&'));
    (segments, query)
}

/// Every `<name>` in these fragments, with Rocket's `..` marker removed.
fn dynamic_names<'a>(fragments: impl Iterator<Item = &'a str>) -> Vec<String> {
    fragments
        .filter_map(|fragment| {
            let start = fragment.find('<')? + 1;
            let end = fragment[start..].find('>')? + start;
            Some(fragment[start..end].trim_end_matches('.').to_owned())
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

    /// Every typed argument of the signature, guards included.
    pub arguments: Vec<HandlerArgument>,
    /// What the route attribute binds, or `None` when the handler carries no
    /// route attribute.
    pub route: Option<Route>,
}

impl AnnotatedHandler {
    /// The handler argument called `name`, if it binds one.
    pub fn argument(&self, name: &str) -> Option<&HandlerArgument> {
        self.arguments
            .iter()
            .find(|argument| argument.ident == name)
    }

    /// The request body the route binds, as `(the argument's type, the body)`.
    ///
    /// `None` when the route declares no `data = "…"` argument, when the named
    /// argument is not one of the signature's, or when the argument binds no body.
    pub fn route_body(&self) -> Option<&HandlerArgument> {
        let data = self.route.as_ref()?.data.as_ref()?;
        let argument = self.argument(&data.value)?;
        (!matches!(argument.body(), BodyBinding::None)).then_some(argument)
    }

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

    /// How many parameters the annotation declares in the form B1 and B2 read.
    pub declared_parameters: usize,
    /// How many parameters the annotation declares in a form they do not read.
    /// Pinned so that the first `IntoParams` struct in the tree fails a test
    /// rather than quietly narrowing the rules.
    pub unread_parameters: usize,
    /// How many request bodies the annotation declares.
    pub request_bodies: usize,
}

/// Does this attribute spell `#[utoipa::path(...)]`?
fn is_utoipa_path(attribute: &Attribute) -> bool {
    let segments = &attribute.path().segments;
    segments.len() == 2 && segments[0].ident == "utoipa" && segments[1].ident == "path"
}

/// The last path segment of a type, when the type is written as a path.
///
/// `Option<GuardAuth>` is not a path type and yields `None`; `GuardResult<X>` is
/// read from its head segment, which is all the rules need.
fn type_head_segment(ty: &Type) -> Option<&syn::Ident> {
    match ty {
        Type::Path(path) => path.path.segments.last().map(|s| &s.ident),
        _ => None,
    }
}

/// Render a type as written, so a finding names the guard rather than a
/// position in a signature: `GuardResult<GuardAuth>`, `GuardAuth`.
fn type_text(ty: &Type) -> String {
    let Type::Path(path) = ty else {
        return type_head_segment(ty).map_or_else(|| "?".to_owned(), ToString::to_string);
    };
    if path.qself.is_some() {
        return "?".to_owned();
    }
    let head: Vec<String> = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    let payload = match path.path.segments.last().map(|s| &s.arguments) {
        Some(PathArguments::AngleBracketed(arguments)) => arguments.args.iter().find_map(|arg| {
            if let GenericArgument::Type(inner) = arg {
                Some(inner)
            } else {
                None
            }
        }),
        _ => None,
    };
    match payload {
        Some(inner) => format!("{}<{}>", head.join("::"), type_text(inner)),
        None => head.join("::"),
    }
}

/// Read every typed argument of one function's signature.
///
/// Patterns that are not a plain name are skipped: a handler that destructures
/// its parameters binds nothing a rule can name, and it cannot be one of this
/// repository's handlers.
fn handler_arguments(handler: &ItemFn) -> Vec<HandlerArgument> {
    handler
        .sig
        .inputs
        .iter()
        .filter_map(|arg| match arg {
            FnArg::Typed(typed) => Some((typed.pat.as_ref(), &typed.ty)),
            FnArg::Receiver(_) => None,
        })
        .filter_map(|(pat, ty)| {
            let ident = match pat {
                Pat::Ident(pat_ident) => &pat_ident.ident,
                _ => return None,
            };
            Some(HandlerArgument {
                ident: ident.to_string(),
                ty: type_text(ty),
                span: pat.span(),
            })
        })
        .collect()
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
        .map(|handler| {
            let arguments = handler_arguments(handler);
            AnnotatedHandler {
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

                arguments,
                route: route_attribute(handler),
            }
        })
        .collect()
}

/// Parse one source file and return its `#[utoipa::path]`-annotated functions.
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

/// A1 to A7 of section A and B1 to B4 of section B: every rule over one handler,
/// in a fixed order so a report reads the same way twice.
fn rules_over(handler: &AnnotatedHandler) -> Vec<Finding> {
    [
        restated_route(handler),
        responses_declared(handler),
        one_vocabulary_tag(handler),
        doc_comment_present(handler),
        one_line_summary(handler),
        no_hand_set_operation_id(handler),
        no_hand_set_prose(handler),
        declared_parameters_bind(handler),
        declared_optionality_agrees(handler),
        declared_body_matches_binding(handler),
        form_body_declares_multipart(handler),
    ]
    .concat()
}

/// B1 — every declared parameter's name is one the route actually binds.
///
/// `rocket_extras` derives a parameter for every argument the route binds, and
/// merges whatever the annotation declares on top. Nothing checks the other
/// direction, so an annotation can declare a parameter no route reads and the
/// document advertises it: a generated client sends it, the server ignores it.
///
/// A header or cookie is out of scope — a Rocket route attribute names neither,
/// so there is nothing to disagree with. So is a handler that carries no route
/// attribute at all: it is not mounted, which `--check-openapi` reports, and a
/// rule here would send the reader to a parameter instead.
fn declared_parameters_bind(handler: &AnnotatedHandler) -> Vec<Finding> {
    handler
        .annotation
        .params
        .iter()
        .filter_map(|declared| {
            let route = handler.route.as_ref()?;
            let (bound, written) = match declared.location {
                ParameterLocation::Path => (&route.path_segments, "path"),
                ParameterLocation::Query => (&route.query, "query"),
                ParameterLocation::Header | ParameterLocation::Cookie => return None,
            };
            if bound.iter().any(|name| name == &declared.name) {
                return None;
            }
            let path = route
                .path
                .as_ref()
                .map_or_else(String::new, |path| path.value.clone());
            Some(Finding::at(
                &handler.file,
                declared.line,
                &handler.name,
                &format!(
                    "the annotation declares the {written} parameter \"{}\", but the route \
                     binds no such {written} parameter: {}",
                    declared.name,
                    describe_route(path, written)
                ),
            ))
        })
        .collect()
}

/// What the route says its `{written}` part is, in the words a reader needs.
fn describe_route(path: String, written: &str) -> String {
    let (head, tail) = path.split_once('?').unwrap_or((&path, ""));
    match written {
        "path" => format!("its path is \"{head}\""),
        _ => match tail.is_empty() {
            true => format!("its path \"{head}\" declares no query part"),
            false => format!("its query part is \"?{tail}\""),
        },
    }
}

/// B2 — a declared parameter's documented `required` agrees with the handler
/// argument it names.
///
/// utoipa 5.5 has no `required` key in a parameter tuple — it rejects the
/// attribute as unknown — and derives the documented `required` from the declared
/// type alone: `Option<…>` is optional, anything else is required. So the two
/// optionalities that have to agree are the declared type's and the handler
/// argument's, and this compares them.
///
/// The dangerous direction is a declared `Option<T>` on a `T` argument: the
/// document says a caller may omit it, the route will not parse without it.
fn declared_optionality_agrees(handler: &AnnotatedHandler) -> Vec<Finding> {
    handler
        .annotation
        .params
        .iter()
        .filter_map(|declared| {
            let argument = handler.argument(&declared.name)?;
            if argument.is_optional() == declared.declared_optional {
                return None;
            }
            Some(Finding::at(
                &handler.file,
                declared.line,
                &handler.name,
                &format!(
                    "the annotation declares the parameter \"{}\" as {}, which utoipa documents \
                     as {}, but the handler binds it as {}, so the document and the route \
                     disagree about whether a caller may omit it",
                    declared.name,
                    declared.declared_type,
                    if declared.declared_optional {
                        "not required"
                    } else {
                        "required"
                    },
                    argument.ty
                ),
            ))
        })
        .collect()
}

/// B3 — a declared `request_body` names the type the route's `data = "…"` binds.
///
/// The declaration is the document's claim and the binding is what Rocket
/// parses; utoipa takes the declaration and never compares the two, so an
/// annotation can advertise a body the route will reject every time.
///
/// Two stated limits, both properties of utoipa's grammar rather than choices:
///
/// - A **`Form<…>` binding is not compared.** Its payload carries `TempFile<'r>`
///   and a lifetime, so no schema type names it. B4 checks what can be checked
///   about a form body — the media type.
/// - **`request_body = Value` declares no constraint** and is not compared. It
///   is utoipa's "any body" and says nothing a route could contradict, so
///   comparing it would only reject a deliberate looseness. The other direction
///   is still a finding: naming a concrete type on a route that binds `Json<Value>`
///   is a claim about what the route parses, and it is false.
///
/// Types are compared by the last segment of their path, which is the name
/// utoipa publishes them under — see [`schema_name`].
fn declared_body_matches_binding(handler: &AnnotatedHandler) -> Vec<Finding> {
    let Some(binding) = handler.route_body() else {
        return Vec::new();
    };
    let BodyBinding::Json(bound) = binding.body() else {
        return Vec::new();
    };
    let Some(declared) = &handler.annotation.request_body else {
        return Vec::new();
    };
    let Some(name) = declared.value.type_name() else {
        return Vec::new();
    };
    if declares_any_body(name) || schema_name(name) == schema_name(&bound) {
        return Vec::new();
    }
    vec![Finding::at(
        &handler.file,
        declared.line,
        &handler.name,
        &format!(
            "the annotation declares request_body = {name}, but the route binds the body as \
             Json<{bound}>, so the document advertises a schema the route never parses"
        ),
    )]
}

/// Does a declared request-body type say "any body"?
///
/// utoipa turns `Value` and `serde_json::Value` into an empty schema, which is the
/// specification's way of saying the body is unconstrained. A custom type whose
/// name merely ends in `Value` still has a concrete schema and is compared.
fn declares_any_body(type_as_written: &str) -> bool {
    matches!(
        type_as_written,
        "Value" | "serde_json::Value" | "::serde_json::Value"
    )
}

/// The name a type contributes to the document.
///
/// utoipa keys a component schema by the last segment of the type path, so
/// `crate::model::album::SetAlbumTitle` and `SetAlbumTitle` publish the same
/// schema and B3 compares them as the same type. Nothing in the tree has two
/// types that share a last segment and differ otherwise.
fn schema_name(type_as_written: &str) -> &str {
    let head = head_segment(type_as_written);
    head.rsplit("::").next().unwrap_or(head)
}

/// B4 — a `Form<…>` binding declares `multipart/form-data`.
///
/// A form endpoint takes `multipart/form-data`, and utoipa guesses
/// `application/json` for every named type that is not a primitive — so an
/// annotation that leaves the media type unsaid documents a JSON body for a route
/// that parses a multipart upload, and a generated client sends JSON to it. This
/// is the one rule of section B that fires on the real tree.
///
/// The rule asks the annotation to name the media type rather than reproducing
/// utoipa's guess: the guess is a list of cases (byte arrays are
/// `application/octet-stream`, primitives are `text/plain`), and reimplementing it
/// here would make this crate a second utoipa to keep in step. Anything but an
/// explicit `multipart/form-data` is a finding, and so is a form route with no
/// `request_body` at all.
fn form_body_declares_multipart(handler: &AnnotatedHandler) -> Vec<Finding> {
    let Some(binding) = handler.route_body() else {
        return Vec::new();
    };
    let BodyBinding::Form(form) = binding.body() else {
        return Vec::new();
    };
    if handler
        .annotation
        .body_content_type
        .as_ref()
        .is_some_and(|declared| declared.value == "multipart/form-data")
    {
        return Vec::new();
    }

    let declared = match &handler.annotation.request_body {
        Some(declared) => match declared.value.type_name() {
            Some(name) => format!("request_body = {name}"),
            None => "a request_body schema this tool does not read".to_owned(),
        },
        None => "no request body at all".to_owned(),
    };
    vec![Finding::at(
        &handler.file,
        handler.signature_line(),
        &handler.name,
        &format!(
            "the route binds a form (Form<{form}>), so its body is multipart/form-data, but the \
             annotation declares {declared} and names no multipart/form-data media type, which \
             utoipa documents as application/json; a generated client would send JSON to a form \
             endpoint"
        ),
    )]
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

                declared_parameters: handler.annotation.params.len(),
                unread_parameters: handler.annotation.unread_params,
                request_bodies: usize::from(handler.annotation.request_body.is_some()),
            });
        }
    }

    Ok(report)
}
