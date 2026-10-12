//! AUTH-2 propagation lint: every `GuardResult<T>` request-guard binding must be
//! propagated with `?` or otherwise handled.
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
use rustc_hir::{Body, BodyId, Expr, ExprKind, FnDecl, HirId, PatKind, QPath, StmtKind, TyKind};
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
    lint_store.register_lints(&[AUTH2_GUARD_PROPAGATION]);
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(Auth2GuardPropagation)));
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

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
