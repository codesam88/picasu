//! Source-level checks on the `#[utoipa::path]` annotations.
//!
//! Four sections of `.plan/openapi-annotation-checks.md` are implemented here:
//! **section A**, the eight rules about the annotation's own shape, **section B**,
//! the four rules about what the annotation declares against what the route
//! already says, **C1** / **C1b**, the two rules about the handler body, and
//! **C3**, what a guard obliges the document to say. The security (D, M) rules are
//! a separate increment, except for the mode guard's `405`, which **M2** asserted
//! and **C3** now enforces — one rule, not two that mean the same thing.
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
//! (`internal`) rather than a copy of a path list. Three conventions in the
//! repository are named rather than read, and all three are recorded as
//! conventions with a comment saying where the prose is: `TAGS` mirrors a table in
//! `docs/openapi-generator.md`, `GUARD_CLASSES` mirrors the naming and rejection
//! conventions recorded in the plan, and the guard shapes below are named by the
//! alias the backend writes them with. `GUARD_CLASSES` is the one place a rule
//! reads a **status**, and it is a convention rather than a fact because the
//! pairing is a decision this repository made — read-only mode is not an
//! authentication failure — not something the tool could derive.
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
//! Why the handler-body rules belong in the gate. `GuardResult<T>` is
//! `Result<T, AppError>`: the route hands the handler a value that may be a
//! rejection, and the *handler* is the only place that rejection can become an
//! error response. Drop it and the route serves a request the guard refused — the
//! `84f29aa5` shape, which an earlier, now-deleted analyzer of the same name
//! caught through a hand-written `AUTH_POLICY` table. A plain `GuardAuth` is the
//! opposite case: Rocket runs it during request handling and short-circuits on
//! failure, so the handler legitimately never touches the value. Treating the two
//! alike would report every handler that correctly binds one as broken.
//!
//! Why the guard-naming and guard-status rules belong in the gate. A8 and C3 read
//! two things nothing else compares: the name a binding is given, and the statuses
//! `responses(…)` declares. A guard parameter named `auth` that binds `GuardShare`
//! makes this tool's own C1 findings — which name the guard by its type, because
//! the name is what it is checking — read as though they were about a token, and a
//! route that can answer `405` because the build is read-only while the annotation
//! lists only `200` and `400` is a document that describes an operation which
//! cannot fail the way it fails.
//!
//! A8's second branch is the one thing here that is not a flat assertion. Rocket
//! binds a route's `?<name>` to a handler argument of the same name, so a query
//! parameter can occupy a guard's canonical name — and `?<timestamp>` occupies it in
//! **every** signature that binds `GuardTimestamp`. Where the canonical name is
//! taken, the binding must still carry it as a word-part. That is an exception with
//! a condition rather than an exemption with a hole, and it is counted: see
//! [`HandlerSummary::taken_name_bindings`].
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
//! 4. which of a handler's parameters are guards, what class each belongs to, and
//!    what each guard obliges the body to do ([`Requirement`], [`GuardClass`]),
//! 5. how the body treats a binding ([`Use`]),
//! 6. what the route attribute binds ([`Route`]: its path segments, its query
//!    names and its `data = "…"` body argument),
//! 7. what each handler argument's type is ([`HandlerArgument`]), which is where
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
//! Guard classification is deliberately narrow. `GuardResult<…>` is the only
//! spelling that makes the *handler* responsible for the rejection, so it is
//! the only one that produces an obligation; every other guard-shaped type is
//! reported as [`Requirement::None`] and no rule applies to it. Anything not
//! recognisable as a guard at all yields `None` from [`guard_requirement`] and
//! is out of scope, which keeps a new guard type from failing the build until
//! someone has decided what it means.
//!
//! A8 and C3 add a second, finer classification: [`guard_class`] resolves a
//! binding's type against [`GUARD_CLASSES`] and `None` for a class the table
//! names not. That is the same honesty with one more step: an unnamed class is
//! out of scope for both rules, and `the_router_tree_is_clean` pins the
//! classified bindings against all bindings so the first one fails a test rather
//! than becoming a guard no rule reads. `auth: TimestampGuardModified` in
//! `backend/src/router/auth.rs` is the case in the tree: it is a plain Rocket
//! guard whose name does not begin with `Guard`, so it is not a binding this
//! crate classifies, and A8's scope note says so rather than guessing a class.
//!
//! # What C3 does not cover
//!
//! **The 401 half is absolute, and that is a stated limitation rather than a
//! decision.** It will fire on a route that deliberately answers a credential
//! rejection with a different status — a `403` for an expired share, say. No
//! mechanism says "this one is meant", and **none is built**, because no route in
//! this repository needs one: all 39 credential-guard handlers reaching the
//! document declare a 401, and the two that did not were the test-only probes,
//! fixed by adding one. The trigger for revisiting it is the first route whose
//! credential rejection is deliberately not a 401.
//!
//! # Pinned shapes
//!
//! Two shapes are not named by C1 and had no precedent in the tree. They are
//! pinned as tests rather than left to be inferred from the walker, and neither
//! expectation may be changed without a rule change going through review:
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
use syn::visit::{self, Visit};
use syn::{
    Attribute, Block, Expr, ExprCall, ExprLet, ExprMatch, ExprReturn, ExprTry, FnArg,
    GenericArgument, ItemFn, Lit, Meta, Pat, PathArguments, Stmt, Type,
};

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

/// The guard classes A8 and C3 name, with the two facts each class carries: the
/// name a binding of it is called, and the status a rejection of it is answered
/// with.
///
/// This is the "lightly project-specific" part of the tool, and the module docs
/// say where the line is: a **class** is a naming and status convention, not a
/// backend fact. It names types rather than reading one — no guard here carries a
/// `const fn is_authentication()` — so a guard class the table does not name is
/// out of scope for A8 and C3 rather than guessed at, and the first one to appear
/// fails the `classified_guards` pin instead of silently going unchecked.
///
/// The pair in each row is not derivable from the type name: `GuardReadOnlyMode`
/// rejects with `405 Method Not Allowed` because read-only mode is a server-side
/// setting a client cannot satisfy with any credential, and every other class
/// rejects with `401` because it rejects a caller who cannot prove who they are.
/// The names in the middle column are the convention A8 enforces, and the
/// reasoning is in the plan file.
pub const GUARD_CLASSES: [(&str, &str, u16); 7] = [
    ("GuardAuth", "auth", 401),
    ("GuardTimestamp", "timestamp", 401),
    ("GuardHash", "hash", 401),
    ("GuardHashOriginal", "hash_original", 401),
    ("GuardShare", "share", 401),
    ("GuardUpload", "upload", 401),
    ("GuardReadOnlyMode", "read_only_mode", 405),
];

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

/// What a guard-shaped parameter obliges the handler body to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    /// `GuardResult<…>`: the value *is* a rejection the handler has to hand on,
    /// so the body must consume it in a way that can fail.
    PropagateRejection,
    /// A plain Rocket request guard: Rocket runs it during request handling and
    /// short-circuits on failure, so the handler legitimately never touches the
    /// value. Nine handler parameters bind such a guard today and none of them
    /// touches it.
    None,
}

/// How the body of a handler treats one binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    /// The binding never appears in the body.
    Absent,
    /// At least one occurrence hands the rejection to the caller.
    Propagated,
    /// The binding appears, but only in positions that discard it. Carries the
    /// line of the first such occurrence, which is where the reader has to look.
    Discarded { line: usize },
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
    /// The `status` of every entry `responses(…)` declares, as written. C3 asks
    /// whether the status a guard rejects with is among them, which needs the
    /// statuses and not only the count.
    pub response_statuses: Vec<String>,
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
                            annotation
                                .response_statuses
                                .extend(response_statuses(&group.stream()));
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

/// The `status` of every entry `responses(…)` declares.
///
/// Each entry is read on its own — descending into the parenthesis utoipa writes
/// it as — rather than by scanning the whole group for a `status` key, so that a
/// nested group inside an entry cannot contribute a status that is not the
/// response's own. A status is kept as written, so `405` compares equal to `405`,
/// and an entry spelled with `default` instead of `status` contributes nothing.
///
/// A rule that read fewer statuses than were declared would find _more_ missing
/// ones, not fewer, so this cannot go quiet the way an unread count can: the first
/// annotation that declares no status at all turns into a finding.
fn response_statuses(stream: &TokenStream) -> Vec<String> {
    split_entries(stream)
        .into_iter()
        .filter_map(|entry| {
            let mut trees: Box<dyn Iterator<Item = TokenTree>> = match entry.into_iter().next() {
                // `(status = 200, description = "Ok")`
                Some(TokenTree::Group(group)) => Box::new(group.stream().into_iter()),
                _ => Box::new(std::iter::empty()),
            };
            while let Some(tree) = trees.next() {
                let TokenTree::Ident(key) = &tree else {
                    continue;
                };
                if key != "status" {
                    continue;
                }
                if !matches!(trees.next(), Some(TokenTree::Punct(punct)) if punct.as_char() == '=')
                {
                    continue;
                }
                let Some(TokenTree::Literal(status)) = trees.next() else {
                    return None;
                };
                return Some(literal_text(&status));
            }
            None
        })
        .collect()
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
/// The test is on the head segment rather than on a prefix, so `Option<T>` is
/// optional and a type merely named `OptionalThing` is not.
fn type_is_optional(text: &str) -> bool {
    head_segment(text) == "Option"
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

/// One guard class of [`GUARD_CLASSES`], resolved from a binding's guard type.
///
/// `Copy` because a handler's classifications are read once per rule and there is
/// nothing to own: every field is a `'static` from the table or a `u16`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuardClass {
    /// The guard type's name as written, which is what a finding names.
    pub type_name: &'static str,
    /// The name a binding of this class is called, which is what A8 expects.
    pub binding: &'static str,
    /// The status a rejection of this class is answered with, which is what C3
    /// expects the annotation to document.
    pub rejection_status: u16,
}

/// One parameter of an annotated handler that is a guard.
#[derive(Debug, Clone)]
pub struct GuardBinding {
    /// The parameter name, without a leading underscore.
    pub ident: String,
    /// Where the parameter is written in the signature.
    pub span: proc_macro2::Span,
    /// The guard's own type as written (`GuardAuth`, `GuardResult<GuardShare>`).
    pub guard_type: String,
    /// What this binding obliges the body to do.
    pub requirement: Requirement,
    /// The class [`GUARD_CLASSES`] resolves this type to, or `None` when the
    /// table names no such class — a guard type A8 and C3 do not speak about.
    pub class: Option<GuardClass>,
}

/// A function carrying a `#[utoipa::path]` annotation, with its annotation, its
/// doc comment, its guard parameters and its body resolved.
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
    pub guards: Vec<GuardBinding>,
    /// Every typed argument of the signature, guards included.
    pub arguments: Vec<HandlerArgument>,
    /// What the route attribute binds, or `None` when the handler carries no
    /// route attribute.
    pub route: Option<Route>,
    pub body: syn::Block,
}

impl AnnotatedHandler {
    /// The `GuardResult<…>` bindings this handler is obliged to propagate.
    pub fn fallible_guards(&self) -> impl Iterator<Item = &GuardBinding> {
        self.guards
            .iter()
            .filter(|guard| guard.requirement == Requirement::PropagateRejection)
    }

    /// The guard classes this handler's route carries, one entry per class.
    ///
    /// Deduplicated because C3 asks a question about the operation's `responses`,
    /// not about each binding: a route carrying two guards of the same class is
    /// missing that class's status once, and reporting it twice would be two
    /// findings for one defect.
    pub fn guard_classes(&self) -> Vec<GuardClass> {
        let mut classes: Vec<GuardClass> = Vec::new();
        for class in self.guards.iter().filter_map(|guard| guard.class) {
            if !classes.contains(&class) {
                classes.push(class);
            }
        }
        classes
    }

    /// How many of this handler's guard bindings [`GUARD_CLASSES`] names.
    ///
    /// The gap between this and `self.guards.len()` is the bindings A8 and C3
    /// cannot speak about, and the scan pins the two counts apart.
    pub fn classified_guards(&self) -> usize {
        self.guards
            .iter()
            .filter(|guard| guard.class.is_some())
            .count()
    }

    /// Does some parameter of this signature other than `guard` already carry the
    /// canonical name of `guard`'s class?
    ///
    /// This is what decides which of A8's two branches a binding is judged by. The
    /// comparison drops a leading underscore from both sides, because `_timestamp`
    /// occupies `timestamp` just as surely as `timestamp` does.
    pub fn canonical_name_is_taken(&self, guard: &GuardBinding) -> bool {
        let Some(class) = guard.class else {
            return false;
        };
        self.arguments.iter().any(|argument| {
            argument.ident != guard.ident && argument.ident.trim_start_matches('_') == class.binding
        })
    }

    /// How many of this handler's guard bindings are judged by A8's taken-name
    /// branch rather than its free-name one.
    ///
    /// Pinned in the scan so that the exception is a recorded count rather than a
    /// rule that has quietly stopped applying: see
    /// [`HandlerSummary::taken_name_bindings`].
    pub fn taken_name_bindings(&self) -> usize {
        self.guards
            .iter()
            .filter(|guard| self.canonical_name_is_taken(guard))
            .count()
    }

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

    /// How the body treats one of this handler's bindings.
    pub fn body_use(&self, ident: &str) -> Use {
        body_use(&self.body, ident)
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

/// What a handler contributes to a [`TreeReport`]: its name and the shape of its
/// guard parameters, without the resolved body — the counts are the only reason
/// the report carries handlers at all, and a `syn::Block` is not `Debug`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerSummary {
    /// Path the source was read from.
    pub file: String,
    /// The function name.
    pub name: String,
    /// How many `GuardResult<…>` parameters the handler binds.
    pub fallible_guards: usize,
    /// How many plain `Guard…` parameters the handler binds.
    pub plain_guards: usize,
    /// How many guard parameters the handler binds in all — what A8 reads, and
    /// the sum of the two counts above.
    pub guard_bindings: usize,
    /// How many of those bindings [`GUARD_CLASSES`] names — what A8 and C3 read.
    /// Pinned against `guard_bindings`, so a guard class neither rule can classify
    /// fails a test instead of going unchecked.
    pub classified_guards: usize,
    /// How many bindings A8 judges by its **taken-name** branch, because another
    /// parameter of the same signature already holds the canonical name. Pinned
    /// like `unread_parameters`: today all four are the `?<timestamp>` collision
    /// in `get_data.rs` and `get_metadata.rs`, so a count that moves means a route
    /// changed and the amendment's scope has to be looked at again.
    pub taken_name_bindings: usize,
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

/// What a parameter type obliges the handler to do, or `None` when the type is
/// not a guard at all and therefore out of scope for every guard rule.
///
/// `GuardResult<…>` is the only obligation. It is spelled as a path whose head
/// is `GuardResult`; the payload is irrelevant here because the rules treat all
/// guard classes alike.
pub fn guard_requirement(ty: &Type) -> Option<Requirement> {
    requirement_of(&type_text(ty))
}

/// The obligation a guard type as written carries.
///
/// The text form is what both the signature and a declared parameter give, so
/// this is the one place the two spellings are decided.
fn requirement_of(type_as_written: &str) -> Option<Requirement> {
    let head = head_segment(type_as_written);
    if head == "GuardResult" {
        return Some(Requirement::PropagateRejection);
    }
    if head.starts_with("Guard") {
        return Some(Requirement::None);
    }
    None
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

/// Read the guard parameters of one function, from the arguments already read.
fn guard_bindings(arguments: &[HandlerArgument]) -> Vec<GuardBinding> {
    arguments
        .iter()
        .filter_map(|argument| {
            let requirement = requirement_of(&argument.ty)?;
            Some(GuardBinding {
                ident: argument.ident.clone(),
                span: argument.span,
                guard_type: argument.ty.clone(),
                requirement,
                class: guard_class(&argument.ty),
            })
        })
        .collect()
}

/// The guard class a guard type as written resolves to.
///
/// `GuardResult<GuardShare>` is classified by its payload, because the alias is
/// what makes the handler responsible for the rejection and says nothing about
/// which guard rejects; the bare `GuardShare` is classified as itself. Both are
/// matched on the type's **last** path segment, so `crate::auth::GuardAuth` and
/// `GuardAuth` are the same class and `GuardHash` is not `GuardHashOriginal`.
///
/// `None` for a type the table does not name. That is the honest answer rather
/// than a guess at a class, and it is why the classified-binding count is pinned:
/// an unnamed class is out of scope for A8 and C3, and the pin makes the first one
/// a test failure rather than a guard nobody reads.
pub fn guard_class(guard_type_as_written: &str) -> Option<GuardClass> {
    let text = guard_type_as_written.replace(' ', "");
    let name = match head_segment(&text) {
        "GuardResult" => text.split_once('<')?.1.trim_end_matches('>'),
        _ => text.as_str(),
    };
    let name = schema_name(name);
    GUARD_CLASSES
        .iter()
        .find_map(|&(type_name, binding, status)| {
            (type_name == name).then_some(GuardClass {
                type_name,
                binding,
                rejection_status: status,
            })
        })
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
                guards: guard_bindings(&arguments),
                arguments,
                route: route_attribute(handler),
                body: handler.block.as_ref().clone(),
            }
        })
        .collect()
}

/// Parse one source file and return its `#[utoipa::path]`-annotated functions
/// with their guard parameters and bodies resolved.
pub fn handlers_in_file(file: &str, source: &str) -> Result<Vec<AnnotatedHandler>, syn::Error> {
    let parsed: syn::File = syn::parse_file(source)?;
    Ok(annotated_handlers(file, &parsed))
}

/// Is this expression exactly the bare identifier `name`?
fn is_ident_expr(expr: &Expr, name: &str) -> bool {
    match expr {
        Expr::Path(path) => {
            path.qself.is_none() && path.path.get_ident().is_some_and(|ident| ident == name)
        }
        // Parenthesised and grouped forms are the same expression to a reader.
        Expr::Paren(paren) => is_ident_expr(&paren.expr, name),
        Expr::Group(group) => is_ident_expr(&group.expr, name),
        _ => false,
    }
}

/// Collects how the body of a handler treats one binding.
struct Uses {
    name: String,
    propagated: bool,
    first_discard: Option<usize>,
}

impl Uses {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            propagated: false,
            first_discard: None,
        }
    }

    /// An occurrence that cannot fail: the rejection dies here.
    fn discard(&mut self, span: proc_macro2::Span) {
        if !self.propagated {
            self.first_discard.get_or_insert(span.start().line);
        }
    }

    /// An occurrence that hands the rejection on to the caller.
    fn propagate(&mut self) {
        self.propagated = true;
    }
}

impl Visit<'_> for Uses {
    fn visit_expr_path(&mut self, node: &syn::ExprPath) {
        if node.qself.is_none()
            && node
                .path
                .get_ident()
                .is_some_and(|ident| ident == self.name.as_str())
        {
            self.discard(node.span());
        }
        visit::visit_expr_path(self, node);
    }

    // `ident?` — the house idiom, and the only shape the tree uses today.
    fn visit_expr_try(&mut self, node: &ExprTry) {
        if is_ident_expr(&node.expr, &self.name) {
            self.propagate();
        }
        visit::visit_expr_try(self, node);
    }

    // `if let … = ident` and `let … = ident` in expression position.
    fn visit_expr_let(&mut self, node: &ExprLet) {
        if is_ident_expr(&node.expr, &self.name) {
            self.propagate();
        }
        visit::visit_expr_let(self, node);
    }

    // `match ident { … }`
    fn visit_expr_match(&mut self, node: &ExprMatch) {
        if is_ident_expr(&node.expr, &self.name) {
            self.propagate();
        }
        visit::visit_expr_match(self, node);
    }

    // `return ident`
    fn visit_expr_return(&mut self, node: &ExprReturn) {
        if node
            .expr
            .as_deref()
            .is_some_and(|expr| is_ident_expr(expr, &self.name))
        {
            self.propagate();
        }
        visit::visit_expr_return(self, node);
    }

    // `f(ident)` — forwarded to another call.
    fn visit_expr_call(&mut self, node: &ExprCall) {
        if node.args.iter().any(|arg| is_ident_expr(arg, &self.name)) {
            self.propagate();
        }
        visit::visit_expr_call(self, node);
    }
}

/// Classify how a body treats the binding `name`.
///
/// The recognised propagating positions are the ones the plan enumerates: the
/// operand of `?`, the scrutinee of `match` / `let` / `if let`, an argument of
/// another call, and a returned value. A trailing expression that *is* the
/// binding counts as returned. Every other occurrence — including
/// `let _ = ident;`, a field access, and a move into a closure — is a discard:
/// it is evidence that the body uses the binding without ever letting its
/// rejection leave the handler.
fn body_use(block: &Block, name: &str) -> Use {
    let mut uses = Uses::new(name);

    // syn 3 keeps the trailing expression as the last `Stmt::Expr(_, None)`.
    if let Some(Stmt::Expr(tail, None)) = block.stmts.last()
        && is_ident_expr(tail, name)
    {
        uses.propagate();
    }
    uses.visit_block(block);

    if uses.propagated {
        Use::Propagated
    } else if let Some(line) = uses.first_discard {
        Use::Discarded { line }
    } else {
        Use::Absent
    }
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
    let Some(second_line) = handler.doc.get(1) else {
        return Vec::new();
    };
    if handler.summary_lines() == 1 {
        return Vec::new();
    }
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

/// A1 to A7 of section A, B1 to B4 of section B, then C1 and C3: every rule over
/// one handler, in a fixed order so a report reads the same way twice.
///
/// A8 sits with C1 and C3 at the end because all three read the same binding. C1b
/// contributes nothing here for the reason given on [`findings_in_source`].
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
        guard_results_propagate(handler),
        guard_bindings_named_after_class(handler),
        guard_rejection_status_documented(handler),
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
/// utoipa turns both spellings of `serde_json::Value` into an empty schema, which
/// is the specification's way of saying the body is unconstrained.
fn declares_any_body(type_as_written: &str) -> bool {
    head_segment(type_as_written).ends_with("Value")
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

/// C1 — a `GuardResult<…>` argument must have its rejection propagated.
///
/// Conforming: the binding is the operand of `?`, a matched scrutinee, an
/// argument of another call, or a returned value. Findings: the binding is used
/// only in positions that discard the rejection (`let _ = ident;`), or the body
/// never mentions it at all.
fn guard_results_propagate(handler: &AnnotatedHandler) -> Vec<Finding> {
    handler
        .fallible_guards()
        .filter_map(|guard| {
            // A discard is anchored where it happens; an absent binding has
            // nothing in the body to point at, so the signature line it is
            // declared on is where the reader has to start.
            let (line, text) = match handler.body_use(&guard.ident) {
                Use::Propagated => return None,
                Use::Discarded { line } => (
                    line,
                    format!(
                        "the route binds {} as a fallible guard, but the handler body \
                         uses it without ever propagating the rejection, so the guard's \
                         rejection never reaches the caller",
                        guard.guard_type
                    ),
                ),
                Use::Absent => (
                    guard.span.start().line,
                    format!(
                        "the route binds {} as a fallible guard, but the handler body \
                         never mentions it, so the guard's rejection never reaches the \
                         caller",
                        guard.guard_type
                    ),
                ),
            };
            Some(Finding::at(&handler.file, line, &handler.name, &text))
        })
        .collect()
}

/// A8 — every guard binding is named after its guard class, and carries the class
/// name as a word where another parameter already has it.
///
/// A guard parameter's name is what a reader scans for when asking "which guard
/// is this, and does the handler use its value", and it is the only handle this
/// tool has on a binding in a finding: C1 names the guard by its _type_, because
/// the name is what it is checking. So a binding called `auth` on a `GuardShare`
/// answers both questions wrongly at a glance — it looks like the token guard, and
/// it is the share guard.
///
/// **The name, exactly, when it is free.** The canonical name for a class is the
/// middle column of [`GUARD_CLASSES`] — `auth`, `read_only_mode`, `hash`,
/// `hash_original`, `share`, `upload`, `timestamp` — and where no other parameter
/// of the signature has it, the binding must be exactly that.
///
/// **The name as a word, where it is taken.** Rocket binds a route's `?<name>` to a
/// handler argument of the same name, so a query parameter can occupy the canonical
/// name. `GuardTimestamp` collides with `?<timestamp>` in **every** signature in
/// this repository that binds the class, which is why the second branch exists at
/// all: with the canonical name unavailable, the binding must still carry it as a
/// **word-part**. The match ignores underscores and case, so for a canonical
/// `timestamp` the names `timestamp`, `timestamp_guard` and `guard_timestamp` all
/// satisfy the rule while `auth` — this repository's own mistake at
/// `backend/src/router/get/get_data.rs:191` and `:221` — does not. It is a
/// word-part test rather than a prefix or a suffix test, so no arrangement of
/// words passes that does not say which guard it is.
///
/// The branch is a statement about one structural collision, not an escape: a
/// binding whose canonical name is **taken** and which carries none of the class
/// name is still a finding, and `a_canonical_name_already_taken_still_needs_the_class_name`
/// pins that. The count of taken-name bindings is pinned in the scan
/// ([`HandlerSummary::taken_name_bindings`]) so that a future change to those four
/// signatures — a route losing its `?<timestamp>`, say — fails a test rather than
/// leaving an exception nobody has looked at.
///
/// `_`-prefixing is the tree's spelling for a guard whose value the handler never
/// uses (`_auth: GuardAuth`), so a leading underscore is stripped before the name
/// is read. It is only right on a binding that is **not** propagated: an underscore
/// on a guard the body hands on with `?` says the value is discarded, which is the
/// opposite of what the body does.
///
/// **Scope: the guard types of [`GUARD_CLASSES`].** A plain Rocket guard whose
/// name does not start with `Guard` is not a binding this tool classifies at all,
/// so `auth: TimestampGuardModified` in `backend/src/router/auth.rs` is out of
/// scope — a rule that needed the class of a type it cannot recognise would have
/// to guess it, and a guessed rename is worse than a left-alone name. The pin on
/// classified bindings makes the first class A8 cannot name a test failure rather
/// than a silently unchecked guard.
fn guard_bindings_named_after_class(handler: &AnnotatedHandler) -> Vec<Finding> {
    handler
        .guards
        .iter()
        .filter_map(|guard| {
            let class = guard.class?;
            let name = guard.ident.as_str();
            let canonical = class.binding;
            let bare = name.strip_prefix('_').unwrap_or(name);

            // A leading underscore is the discarded-value spelling, so it is only
            // right on a binding the body never propagates.
            let underscore_is_right =
                bare.len() == name.len() || !matches!(handler.body_use(name), Use::Propagated);

            // Two branches, and which one applies is a fact about the signature
            // rather than a choice the rule makes per binding: where the
            // canonical name is free the binding must be exactly it, and where
            // another parameter already has it the binding must still carry it
            // as a word.
            let taken = handler.canonical_name_is_taken(guard);
            let named_after_class = if taken {
                carries_class_name(bare, canonical)
            } else {
                bare == canonical
            };
            if underscore_is_right && named_after_class {
                return None;
            }

            let text = if taken {
                format!(
                    "the guard binding \"{name}\" carries none of its guard class's name: it \
                     binds {}, whose name is \"{canonical}\", and another parameter in this \
                     signature is already called \"{canonical}\", so the binding still has to \
                     carry \"{canonical}\" as a word — \"{canonical}_guard\" and \
                     \"guard_{canonical}\" both do, and \"{name}\" does not",
                    guard.guard_type
                )
            } else {
                format!(
                    "the guard binding \"{name}\" is not named after its guard class: it binds \
                     {}, whose binding is called \"{canonical}\", and the name is what a reader \
                     of the handler, or of any finding this tool reports about it, uses to say \
                     which guard this is",
                    guard.guard_type
                )
            };
            Some(Finding::at(
                &handler.file,
                guard.span.start().line,
                &handler.name,
                &text,
            ))
        })
        .collect()
}

/// Does `name` carry `canonical` as a word-part?
///
/// Underscores are dropped from both sides and case is ignored, so `guard_timestamp`
/// and `timestamp_guard` both carry `timestamp` and neither carries `auth`. This is
/// a containment test rather than a prefix or a suffix one on purpose: what A8
/// requires is that the name _says which guard it is_, and any arrangement of words
/// around the class name says it.
fn carries_class_name(name: &str, canonical: &str) -> bool {
    fold_case_words(name).contains(&fold_case_words(canonical))
}

/// A name with its underscores removed and its letters lowercased, so that the two
/// spellings of the same word compare equal.
fn fold_case_words(name: &str) -> String {
    name.chars()
        .filter(|character| *character != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

/// C3 — a route carrying a guard documents the status that guard rejects with.
///
/// The guard is the route's own statement about who may call it, and the status is
/// the part of that statement a client reads before it makes the call. utoipa
/// publishes exactly the statuses the annotation declares and invents none, so a
/// route that can answer `401` and documents only `200` and `400` produces an
/// operation whose failure modes a generated client cannot see — the same shape as
/// A2's missing `responses(…)`, one level down.
///
/// The mapping is [`GUARD_CLASSES`]' second column. **`GuardReadOnlyMode` is the
/// `405` half, which is what M2 asserted**: a read-only build cannot be lifted by
/// anything a client sends, so the status is "this method is not allowed here"
/// rather than "who are you". M2 is expressed by this rule and is not a second
/// rule saying the same thing.
///
/// The 401 half is the same assertion over the six credential classes. A route can
/// carry two guards of different classes and both statuses are checked; a route
/// carrying two of the same class is reported once, because the missing thing is
/// one entry in `responses(…)`.
///
/// **Stated limitation, recorded rather than designed around.** The 401 half is
/// absolute, so it will fire on a route that deliberately answers a credential
/// rejection with a different status — a `403` for an expired share, say, or a
/// `404` for a share that no longer exists. Such a route is not wrong; the rule
/// has no way to say so. **It does not provide an exemption mechanism, and none is
/// built**, because no route in this repository needs one: the 39 credential-guard
/// handlers that reach the document all declare a 401, and the two that did not
/// were the test-only probes, fixed by adding one. The trigger for revisiting this
/// is therefore concrete: the first route whose credential rejection is
/// deliberately not a 401. What that needs is a decision about how the
/// declaration says so, which is a question about the document rather than about
/// this rule — so it is recorded here and in the plan file rather than answered
/// with a bypass.
fn guard_rejection_status_documented(handler: &AnnotatedHandler) -> Vec<Finding> {
    let statuses = &handler.annotation.response_statuses;
    handler
        .guard_classes()
        .into_iter()
        .filter(|class| {
            let status = class.rejection_status.to_string();
            !statuses.iter().any(|declared| declared == &status)
        })
        .map(|class| {
            Finding::at(
                &handler.file,
                responses_line(handler),
                &handler.name,
                &format!(
                    "the route binds {}, which rejects with {}, but the annotation documents \
                     ({}) and not that status, so the document does not say the operation can \
                     answer it",
                    class.type_name,
                    class.rejection_status,
                    statuses.join(", ")
                ),
            )
        })
        .collect()
}

/// Where a finding about the declared responses points.
///
/// The `responses(…)` group when the annotation declares one, because that is the
/// token a reader has to edit; the signature line when it declares none, which A2
/// reports in its own right and which leaves C3 nothing to point at.
fn responses_line(handler: &AnnotatedHandler) -> usize {
    handler
        .annotation
        .responses
        .as_ref()
        .map_or_else(|| handler.signature_line(), |responses| responses.line)
}

/// Every rule over one source file: the six of section A and the two of section C.
///
/// C1b contributes nothing here: a plain guard carries `Requirement::None`, so
/// `fallible_guards` filters it out before the body is ever consulted.
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
                fallible_guards: handler.fallible_guards().count(),
                plain_guards: handler.guards.len() - handler.fallible_guards().count(),
                guard_bindings: handler.guards.len(),
                classified_guards: handler.classified_guards(),
                taken_name_bindings: handler.taken_name_bindings(),
                declared_parameters: handler.annotation.params.len(),
                unread_parameters: handler.annotation.unread_params,
                request_bodies: usize::from(handler.annotation.request_body.is_some()),
            });
        }
    }

    Ok(report)
}
