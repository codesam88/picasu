//! Source-level checks on the `#[utoipa::path]` annotations.
//!
//! This increment covers the two rules of section C of
//! `.plan/openapi-annotation-checks.md` that are about the handler body — **C1**
//! (a `GuardResult<…>` argument must have its rejection propagated) and **C1b**
//! (a plain `Guard…` argument needs nothing in the body and is never reported).
//! The annotation-shape, parameter-agreement and security rules are separate
//! increments.
//!
//! Why these two belong in the gate. `GuardResult<T>` is `Result<T, AppError>`:
//! the route hands the handler a value that may be a rejection, and the
//! *handler* is the only place that rejection can become an error response. Drop
//! it and the route serves a request the guard refused — the `84f29aa5` shape,
//! which an earlier, now-deleted analyzer of the same name caught through a
//! hand-written `AUTH_POLICY` table. A plain `GuardAuth` is the opposite case:
//! Rocket runs it during request handling and short-circuits on failure, so the
//! handler legitimately never touches the value. Treating the two alike would
//! report every handler that correctly binds one as broken.
//!
//! Both rules read a fact that exists only in source: nothing in the generated
//! document or in the mount table says what a handler body does with a guard.
//!
//! # What the scan answers
//!
//! Every assertion reads something the tests decide what to make of, and all of
//! it comes from parsing the files with `syn` rather than comparing two derived
//! views:
//!
//! 1. which functions carry a `#[utoipa::path]` annotation,
//! 2. which of their parameters are guards, and what each guard obliges the
//!    body to do ([`Requirement`]),
//! 3. how the body treats a binding ([`Use`]).
//!
//! Guard classification is deliberately narrow. `GuardResult<…>` is the only
//! spelling that makes the *handler* responsible for the rejection, so it is
//! the only one that produces an obligation; every other guard-shaped type is
//! reported as [`Requirement::None`] and no rule applies to it. Anything not
//! recognisable as a guard at all yields `None` from [`guard_requirement`] and
//! is out of scope, which keeps a new guard type from failing the build until
//! someone has decided what it means.
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

use std::fmt;
use std::path::{Path, PathBuf};

use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{
    Attribute, Block, Expr, ExprCall, ExprLet, ExprMatch, ExprReturn, ExprTry, FnArg,
    GenericArgument, ItemFn, Pat, PathArguments, Stmt, Type,
};

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
}

/// A function carrying a `#[utoipa::path]` annotation, with its guard
/// parameters and its body resolved.
///
/// No `Debug`: `syn::Block` is only `Debug` under syn's `extra-traits` feature,
/// which this crate does not otherwise need.
#[derive(Clone)]
pub struct AnnotatedHandler {
    /// Path the source was read from, for finding text.
    pub file: String,
    /// The function name — a handler's identity in a finding.
    pub name: String,
    pub guards: Vec<GuardBinding>,
    pub body: syn::Block,
}

impl AnnotatedHandler {
    /// The `GuardResult<…>` bindings this handler is obliged to propagate.
    pub fn fallible_guards(&self) -> impl Iterator<Item = &GuardBinding> {
        self.guards
            .iter()
            .filter(|guard| guard.requirement == Requirement::PropagateRejection)
    }

    /// How the body treats one of this handler's bindings.
    pub fn body_use(&self, ident: &str) -> Use {
        body_use(&self.body, ident)
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
    let head = type_head_segment(ty)?;
    if head == "GuardResult" {
        return Some(Requirement::PropagateRejection);
    }
    if head.to_string().starts_with("Guard") {
        return Some(Requirement::None);
    }
    None
}

/// Read the guard parameters of one function.
fn guard_bindings(handler: &ItemFn) -> Vec<GuardBinding> {
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
            let requirement = guard_requirement(ty)?;
            Some(GuardBinding {
                ident: ident.to_string(),
                span: pat.span(),
                guard_type: type_text(ty),
                requirement,
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
        .map(|handler| AnnotatedHandler {
            file: file.to_owned(),
            name: handler.sig.ident.to_string(),
            guards: guard_bindings(handler),
            body: handler.block.as_ref().clone(),
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

/// Both rules over one source file.
///
/// C1b contributes nothing here: a plain guard carries `Requirement::None`, so
/// `fallible_guards` filters it out before the body is ever consulted.
pub fn findings_in_source(name: &str, source: &str) -> Result<Vec<Finding>, ScanError> {
    handlers_in_file(name, source)
        .map_err(|error| ScanError {
            file: name.to_owned(),
            message: format!("must parse as Rust: {error}"),
        })
        .map(|handlers| handlers.iter().flat_map(guard_results_propagate).collect())
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

/// Both rules over every `.rs` file under `source_root`.
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
            report.findings.extend(guard_results_propagate(handler));
            report.handlers.push(HandlerSummary {
                file: handler.file.clone(),
                name: handler.name.clone(),
                fallible_guards: handler.fallible_guards().count(),
                plain_guards: handler.guards.len() - handler.fallible_guards().count(),
            });
        }
    }

    Ok(report)
}
