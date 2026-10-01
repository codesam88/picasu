//! Source-level scanning support for the `#[utoipa::path]` annotation checks.
//!
//! Every assertion in [`super::openapi_annotations`] reads a fact that exists
//! only in the source tree — the shape of an annotation, the type of a handler
//! argument, what the handler body does with a binding. Nothing in the
//! generated document or in Rocket's mount table carries any of it, so this
//! module parses the files with `syn` instead of comparing two derived views.
//!
//! It holds no rules. The scan answers three questions and the tests in
//! [`super::openapi_annotations`] decide what an answer costs:
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

use std::fmt;
use std::path::Path;

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
    /// Path as written by the scan, relative to the crate root where possible.
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

/// Render findings for an assertion message, one per line, sorted by location
/// so a diff between two runs shows only what moved.
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
/// which this scan does not otherwise need.
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
pub fn body_use(block: &Block, name: &str) -> Use {
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

/// Name a source file for a finding, relative to the crate root.
///
/// Without this a finding would carry the absolute path of whichever worktree
/// the test ran in, which differs between machines and between a local run and
/// CI.
pub fn relative_name(path: &Path, manifest_dir: &Path) -> String {
    path.strip_prefix(manifest_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
