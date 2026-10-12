//! AUTH-3 no-forward-denial lint: no `Outcome::Forward` inside a
//! `FromRequest` guard impl — denial must be `Outcome::Error`.
//!
//! Detection is type-resolved, not textual:
//! - the enclosing impl is confirmed as a `FromRequest` impl by resolved
//!   trait, not by name; inside it, an `Outcome::Forward` path matches iff
//!   its `qself` resolves to the `Outcome` enum (`QPath::TypeRelative`, the
//!   shape hand-written code takes) or it resolves via its `Ctor` def to an
//!   `Outcome` variant — or, fail closed, its spelling ends in
//!   `Outcome::Forward`. Rocket-generated route shims also mention
//!   `Outcome::Forward`, but those live outside any `FromRequest` impl and
//!   never match.

#![feature(rustc_private)]

extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_middle;

use rustc_errors::DiagDecorator;
use rustc_hir::def::{DefKind, Res};
use rustc_hir::{Expr, ExprKind, HirId, ItemKind, Node, QPath, TyKind};
use rustc_hir::def_id::DefId;
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty::TyCtxt;

dylint_linting::dylint_library!();

extern crate rustc_lint;
extern crate rustc_session;

#[unsafe(no_mangle)]
pub fn register_lints(sess: &rustc_session::Session, lint_store: &mut rustc_lint::LintStore) {
    dylint_linting::init_config(sess);
    lint_store.register_lints(&[AUTH3_NO_FORWARD_DENIAL]);
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(Auth3NoForwardDenial)));
}

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

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
