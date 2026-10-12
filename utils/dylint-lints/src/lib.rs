//! AUTH-2 propagation lint: every `GuardResult<T>` request-guard binding must be
//! propagated with `?` or otherwise handled.
//!
//! AUTH-3 no-forward-denial lint: no `Outcome::Forward` inside a
//! `FromRequest` guard impl — denial must be `Outcome::Error`.
//!
//! Detection is type-resolved, not textual:
//! - a handler parameter counts as a guard binding iff its declared type
//!   resolves (HIR `Res::Def`) to the `router::GuardResult` type alias **and**
//!   the body's typeck results say the binding's type is `core::result::Result`
//!   (diagnostic item, via `TyCtxt`). A same-spelled lookalike alias, or an
//!   alias over a non-`Result` type, cannot match;
//! - a use counts as propagation iff it is anything other than a direct
//!   `let _ = binding;` discard (or an explicit `drop(binding)`).
//!
//! `?` needs no special case: `x?` desugars to `match branch(x) { ... }`, so the
//! binding use sits under a `branch(..)` call, never directly under a
//! `let _ = ...`. `let _ = x?;` therefore passes, `let _ = x;` fails.

#![feature(rustc_private)]

extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_middle;
extern crate rustc_span;

use rustc_errors::DiagDecorator;
use rustc_hir::def::{DefKind, Res};
use rustc_hir::intravisit::{self, FnKind, Visitor};
use rustc_hir::{Body, BodyId, Expr, ExprKind, FnDecl, HirId, ItemKind, Node, PatKind, QPath, StmtKind, TyKind};
use rustc_hir::def_id::{DefId, LocalDefId};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty::{self, TyCtxt};
use rustc_span::Span;

dylint_linting::dylint_library!();

extern crate rustc_lint;
extern crate rustc_session;

#[unsafe(no_mangle)]
pub fn register_lints(sess: &rustc_session::Session, lint_store: &mut rustc_lint::LintStore) {
    dylint_linting::init_config(sess);
    lint_store.register_lints(&[AUTH2_GUARD_PROPAGATION, AUTH3_NO_FORWARD_DENIAL]);
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(Auth2GuardPropagation)));
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(Auth3NoForwardDenial)));
}

rustc_session::declare_lint! {
    /// ### What it does
    ///
    /// Flags handler parameters of type `GuardResult<T>` whose value is never
    /// propagated (`?`) nor otherwise handled — i.e. the only uses are
    /// `let _ = binding;` discards or there are no uses at all.
    pub AUTH2_GUARD_PROPAGATION,
    Warn,
    "a `GuardResult` guard binding is discarded without propagation"
}

rustc_session::declare_lint_pass!(Auth2GuardPropagation => [AUTH2_GUARD_PROPAGATION]);

rustc_session::declare_lint! {
    /// ### What it does
    ///
    /// AUTH-3 ("Denial is an error, not a fall-through"): flags every
    /// `Outcome::Forward` expression inside a `FromRequest::from_request`
    /// impl. A guard that rejects must return `Outcome::Error` with the
    /// declared status; `Outcome::Forward` only selects the next ranked
    /// route, never denies.
    ///
    /// Detection: the enclosing impl is confirmed as a `FromRequest` impl by
    /// resolved trait, not by name; inside it, an `Outcome::Forward` path
    /// matches iff its `qself` resolves to the `Outcome` enum
    /// (`QPath::TypeRelative`, the shape hand-written code takes) or it
    /// resolves via its `Ctor` def to an `Outcome` variant — or, fail
    /// closed, its spelling ends in `Outcome::Forward`. Rocket-generated
    /// route shims also mention `Outcome::Forward`, but those live outside
    /// any `FromRequest` impl and never match.
    pub AUTH3_NO_FORWARD_DENIAL,
    Warn,
    "`Outcome::Forward` used inside a `FromRequest` guard impl (denial must be `Outcome::Error`)"
}

rustc_session::declare_lint_pass!(Auth3NoForwardDenial => [AUTH3_NO_FORWARD_DENIAL]);

fn def_path_names(tcx: TyCtxt<'_>, did: DefId) -> Vec<String> {
    tcx.def_path(did)
        .data
        .iter()
        .filter_map(|elem| elem.data.get_opt_name().map(|s| s.to_string()))
        .collect()
}

/// The declared type spells `router::GuardResult<..>` (compiler-resolved alias).
fn is_guard_result_spelling(tcx: TyCtxt<'_>, ty: &rustc_hir::Ty<'_>) -> bool {
    if let TyKind::Path(QPath::Resolved(None, path)) = &ty.kind {
        if let Res::Def(DefKind::TyAlias, alias_did) = path.res {
            let names = def_path_names(tcx, alias_did);
            return names.len() >= 2
                && names[names.len() - 2] == "router"
                && names[names.len() - 1] == "GuardResult";
        }
    }
    false
}

/// The binding's computed type is `core::result::Result` (diagnostic item).
fn is_result_ty<'tcx>(cx: &LateContext<'tcx>, pat: &rustc_hir::Pat<'_>) -> bool {
    let Some(typeck) = cx.maybe_typeck_results() else {
        return false;
    };
    if let ty::Adt(adt, _) = *typeck.pat_ty(pat).kind() {
        cx.tcx.is_diagnostic_item(rustc_span::sym::Result, adt.did())
    } else {
        false
    }
}

/// How one use of a guard binding treats the value.
#[derive(PartialEq, Eq)]
enum UseKind {
    /// `let _ = binding;` or `drop(binding)`: the value is silently dropped.
    Discard,
    /// `let name = binding;`: a move into a shadowing/aliasing binding. This
    /// includes the compiler-generated `let param = param;` that moves `async`
    /// fn params into the future — not enforcement, but not a discard either.
    Neutral,
    /// Anything else (`?`, match, field access, call argument, ...).
    Propagate,
}

/// Classify one use of a guard binding by its direct parent node.
fn classify_use(tcx: TyCtxt<'_>, use_id: HirId) -> UseKind {
    let parent = tcx.hir_node(tcx.parent_hir_id(use_id));
    if let rustc_hir::Node::LetStmt(let_stmt) = parent {
        if let_stmt.init.is_some_and(|init| init.hir_id == use_id) {
            if matches!(let_stmt.pat.kind, PatKind::Wild) {
                return UseKind::Discard;
            }
            if matches!(let_stmt.pat.kind, PatKind::Binding(..)) {
                return UseKind::Neutral;
            }
        }
    }
    if let rustc_hir::Node::Expr(Expr {
        kind: ExprKind::Call(func, _),
        ..
    }) = parent
    {
        if let ExprKind::Path(QPath::Resolved(None, path)) = &func.kind {
            if let Res::Def(DefKind::Fn, did) = path.res {
                let names = def_path_names(tcx, did);
                if names.len() >= 2 && names[names.len() - 2] == "mem" && names[names.len() - 1] == "drop" {
                    return UseKind::Discard;
                }
            }
        }
    }
    UseKind::Propagate
}

struct UseCollector<'tcx> {
    tcx: TyCtxt<'tcx>,
    /// The param binding plus every shadowing/aliasing binding that directly
    /// re-binds a tracked value (`let b = a;`, including the async-desugar
    /// `let param = param;`). Uses of any of these are uses of the guard.
    tracked: Vec<HirId>,
    uses: Vec<HirId>,
}

impl<'tcx> Visitor<'tcx> for UseCollector<'tcx> {
    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
        if let ExprKind::Path(QPath::Resolved(None, path)) = &expr.kind {
            if let Res::Local(id) = path.res {
                if self.tracked.contains(&id) {
                    self.uses.push(expr.hir_id);
                }
            }
        }
        intravisit::walk_expr(self, expr);
    }

    fn visit_stmt(&mut self, stmt: &'tcx rustc_hir::Stmt<'tcx>) {
        // Follow the shadow chain: `let b = a;` (binding pat, direct local
        // init) makes `b` another name for the guard value.
        if let StmtKind::Let(let_stmt) = &stmt.kind {
            if let PatKind::Binding(_, shadow, ..) = let_stmt.pat.kind {
                if let Some(init) = let_stmt.init {
                    if let ExprKind::Path(QPath::Resolved(None, path)) = &init.kind {
                        if let Res::Local(id) = path.res {
                            if self.tracked.contains(&id) && !self.tracked.contains(&shadow) {
                                self.tracked.push(shadow);
                            }
                        }
                    }
                }
            }
        }
        intravisit::walk_stmt(self, stmt);
    }

    fn visit_nested_body(&mut self, id: BodyId) {
        intravisit::walk_body(self, self.tcx.hir_body(id));
    }
}

/// Def path names a `FromRequest` trait under a `request` module
/// (`rocket::request::FromRequest`, currently
/// `rocket::request::from_request::FromRequest`) — resolved, not textual.
fn is_from_request_trait(tcx: TyCtxt<'_>, did: DefId) -> bool {
    let names = def_path_names(tcx, did);
    names.len() >= 2
        && names[names.len() - 1] == "FromRequest"
        && names[..names.len() - 1].iter().any(|n| n == "request")
}

/// The enclosing HIR item of `hir_id` is an impl of the (resolved)
/// `request::FromRequest` trait.
fn in_from_request_impl(tcx: TyCtxt<'_>, hir_id: HirId) -> bool {
    for (_, node) in tcx.hir_parent_iter(hir_id) {
        if let Node::Item(item) = node {
            if let ItemKind::Impl(impl_) = &item.kind {
                if let Some(header) = impl_.of_trait {
                    if let Res::Def(DefKind::Trait, did) = header.trait_ref.path.res {
                        return is_from_request_trait(tcx, did);
                    }
                }
                return false;
            }
        }
    }
    false
}

/// Path segments end in `Outcome::Forward` (covers fully-qualified spellings).
fn spells_outcome_forward(path: &rustc_hir::Path<'_>) -> bool {
    let n = path.segments.len();
    n >= 2
        && path.segments[n - 2].ident.as_str() == "Outcome"
        && path.segments[n - 1].ident.as_str() == "Forward"
}

/// A type spells the `Outcome` enum: resolved to an enum whose def path ends
/// in `Outcome`, or — fail closed — its last path segment is `Outcome`.
fn spells_outcome_enum(tcx: TyCtxt<'_>, ty: &rustc_hir::Ty<'_>) -> bool {
    if let TyKind::Path(qpath) = &ty.kind {
        if let QPath::Resolved(None, path) = qpath {
            if let Res::Def(DefKind::Enum, did) = path.res {
                let names = def_path_names(tcx, did);
                if names.last().is_some_and(|n| n == "Outcome") {
                    return true;
                }
            }
            if let Some(last) = path.segments.last() {
                return last.ident.as_str() == "Outcome";
            }
        }
    }
    false
}

/// The path resolves to a variant of the `request::Outcome` enum.
fn resolves_to_outcome_forward(tcx: TyCtxt<'_>, res: Res) -> bool {
    if let Res::Def(DefKind::Ctor(..), ctor_did) = res {
        // Ctor -> variant -> enum.
        let variant_did = tcx.parent(ctor_did);
        let enum_did = tcx.parent(variant_did);
        let names = def_path_names(tcx, enum_did);
        return names.len() >= 2
            && names[names.len() - 2] == "request"
            && names[names.len() - 1] == "Outcome";
    }
    false
}

impl<'tcx> LateLintPass<'tcx> for Auth3NoForwardDenial {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let ExprKind::Path(ref qpath) = expr.kind else {
            return;
        };
        // Hand-written `Outcome::Forward` resolves as `TypeRelative`
        // (`Outcome` as qself, `Forward` as the segment); fully-qualified
        // spellings appear as `Resolved`.
        let is_forward = match qpath {
            QPath::Resolved(None, path) => {
                resolves_to_outcome_forward(cx.tcx, path.res) || spells_outcome_forward(path)
            }
            QPath::TypeRelative(ty, seg) => {
                seg.ident.as_str() == "Forward" && spells_outcome_enum(cx.tcx, ty)
            }
            _ => false,
        };
        if !is_forward {
            return;
        }
        if !in_from_request_impl(cx.tcx, expr.hir_id) {
            return;
        }
        cx.emit_span_lint(
            AUTH3_NO_FORWARD_DENIAL,
            expr.span,
            DiagDecorator(|diag| {
                diag.primary_message(
                    "`Outcome::Forward` in a `FromRequest` guard impl: denial must be `Outcome::Error`, `Forward` only selects the next ranked route",
                );
            }),
        );
    }
}

impl<'tcx> LateLintPass<'tcx> for Auth2GuardPropagation {
    fn check_fn(
        &mut self,
        cx: &LateContext<'tcx>,
        _: FnKind<'tcx>,
        decl: &'tcx FnDecl<'tcx>,
        body: &'tcx Body<'tcx>,
        _: Span,
        _: LocalDefId,
    ) {
        // `Param` no longer carries its type; `decl.inputs[i]` is the type of
        // `body.params[i]`.
        for (param, input_ty) in body.params.iter().zip(decl.inputs.iter()) {
            let PatKind::Binding(_, binding_id, _, _) = param.pat.kind else {
                continue;
            };
            if !is_guard_result_spelling(cx.tcx, input_ty) {
                continue;
            }
            if !is_result_ty(cx, param.pat) {
                continue;
            }

            let mut collector = UseCollector {
                tcx: cx.tcx,
                tracked: vec![binding_id],
                uses: Vec::new(),
            };
            intravisit::walk_body(&mut collector, body);

            // Fail closed: flag unless at least one use genuinely propagates
            // the value. `Neutral` (async-desugar shadows, aliasing moves)
            // and zero uses never count as propagation.
            let propagated = collector
                .uses
                .iter()
                .any(|use_id| classify_use(cx.tcx, *use_id) == UseKind::Propagate);
            if !propagated {
                cx.emit_span_lint(
                    AUTH2_GUARD_PROPAGATION,
                    param.span,
                    DiagDecorator(|diag| {
                        diag.primary_message(
                            "a `GuardResult` guard binding is discarded without propagation",
                        );
                    }),
                );
            }
        }
    }
}
