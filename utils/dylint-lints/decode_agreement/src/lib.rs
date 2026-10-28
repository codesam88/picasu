//! Decode-agreement lint (AUTH-1/F1 ratchet): each guard must decode exactly
//! the claims type the model assigns it.
//!
//! Detection is type-resolved, not textual:
//! - the enclosing impl is confirmed as a `FromRequest` impl by resolved
//!   trait (same check as `auth3_no_forward_denial`), and the guard identity
//!   is the impl's `Self` type name (`GuardAuth`, `GuardHash`, ...);
//! - each `decode_typed` call inside such an impl is matched by resolved
//!   function (def path ends in `decode_typed`);
//! - the decode target `T` is read from the call's typeck type
//!   (`Result<T, E>` — first generic argument of the `Result` ADT), so both
//!   turbofish (`decode_typed::<ClaimsHash>(..)`) and inferred
//!   (`let claims: ClaimsHash = decode_typed(..)`) spellings resolve to the
//!   same type. Fail closed: if `T` cannot be resolved, the site warns.
//!
//! Expected table (established empirically from `backend/src/router/auth.rs`):
//! `GuardAuth`/`GuardUser` → `Claims`, `GuardHash`/`GuardHashOriginal` →
//! `ClaimsHash`, `TimestampGuardModified`/`GuardTimestamp` → `ClaimsTimestamp`.
//! `GuardShare` resolves (share-table lookup, possibly via
//! `try_jwt_cookie_auth` which decodes `Claims`) rather than decoding
//! directly; `GuardReadOnlyMode` decodes nothing. A direct `decode_typed`
//! call inside any of those impls warns.

#![feature(rustc_private)]

extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_middle;

use rustc_errors::DiagDecorator;
use rustc_hir::def::{DefKind, Res};
use rustc_hir::{Expr, ExprKind, HirId, ItemKind, Node, QPath, TyKind};
use rustc_hir::def_id::DefId;
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty::{self, TyCtxt};

dylint_linting::dylint_library!();

extern crate rustc_lint;
extern crate rustc_session;

#[unsafe(no_mangle)]
pub fn register_lints(sess: &rustc_session::Session, lint_store: &mut rustc_lint::LintStore) {
    dylint_linting::init_config(sess);
    lint_store.register_lints(&[DECODE_AGREEMENT]);
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(DecodeAgreement)));
}

rustc_session::declare_lint! {
    /// ### What it does
    ///
    /// AUTH-1/F1 ("decode agreement"): flags a `decode_typed` call inside a
    /// `FromRequest` guard impl whose decode target differs from the claims
    /// type the model assigns that guard. A guard decoding the wrong claims
    /// type accepts tokens minted for another purpose (type confusion).
    pub DECODE_AGREEMENT,
    Warn,
    "a guard decodes a claims type other than the one the model assigns it"
}

rustc_session::declare_lint_pass!(DecodeAgreement => [DECODE_AGREEMENT]);

fn def_path_names(tcx: TyCtxt<'_>, did: DefId) -> Vec<String> {
    tcx.def_path(did)
        .data
        .iter()
        .filter_map(|elem| elem.data.get_opt_name().map(|s| s.to_string()))
        .collect()
}

fn is_from_request_trait(tcx: TyCtxt<'_>, did: DefId) -> bool {
    let names = def_path_names(tcx, did);
    names.len() >= 2
        && names[names.len() - 1] == "FromRequest"
        && names[..names.len() - 1].iter().any(|n| n == "request")
}

/// If `hir_id` sits inside a `FromRequest` impl, return the impl's `Self`
/// type name (e.g. `GuardHash`).
fn enclosing_guard_name(tcx: TyCtxt<'_>, hir_id: HirId) -> Option<String> {
    for (_, node) in tcx.hir_parent_iter(hir_id) {
        if let Node::Item(item) = node {
            if let ItemKind::Impl(impl_) = &item.kind {
                let header = impl_.of_trait.as_ref()?;
                if let Res::Def(DefKind::Trait, did) = header.trait_ref.path.res {
                    if !is_from_request_trait(tcx, did) {
                        return None;
                    }
                } else {
                    return None;
                }
                if let TyKind::Path(qpath) = &impl_.self_ty.kind {
                    let last = match qpath {
                        QPath::Resolved(_, path) => path.segments.last()?,
                        QPath::TypeRelative(_, seg) => seg,
                    };
                    return Some(last.ident.as_str().to_string());
                }
                return None;
            }
        }
    }
    None
}

/// The model-assigned claims type for a guard, or `None` for guards that
/// must not decode directly (`GuardShare` resolves rather than decoding);
/// `GuardReadOnlyMode`).
fn expected_claims(guard: &str) -> Option<&'static str> {
    match guard {
        "GuardAuth" | "GuardUser" => Some("Claims"),
        "GuardHash" | "GuardHashOriginal" => Some("ClaimsHash"),
        "TimestampGuardModified" | "GuardTimestamp" => Some("ClaimsTimestamp"),
        _ => None,
    }
}

/// Resolved callee is the `decode_typed` free function.
fn is_decode_typed_call(res: Res, tcx: TyCtxt<'_>) -> bool {
    if let Res::Def(kind, did) = res {
        if matches!(kind, DefKind::Fn | DefKind::AssocFn) {
            let names = def_path_names(tcx, did);
            return names.last().is_some_and(|n| n == "decode_typed");
        }
    }
    false
}

/// Decode target `T` of a `decode_typed(..)` call from typeck: the call's
/// type is `Result<T, E>`, so `T` is the first generic argument of the
/// `Result` ADT. Returns the ADT's def-path last name (e.g. `ClaimsHash`).
fn decode_target_name<'tcx>(cx: &LateContext<'tcx>, call: &'tcx Expr<'tcx>) -> Option<String> {
    let typeck = cx.maybe_typeck_results()?;
    let ty = typeck.expr_ty(call);
    if let ty::Adt(_, args) = *ty.kind() {
        let inner = args.type_at(0);
        if let ty::Adt(adt, _) = *inner.kind() {
            let names = def_path_names(cx.tcx, adt.did());
            return names.last().cloned();
        }
    }
    None
}

impl<'tcx> LateLintPass<'tcx> for DecodeAgreement {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        let ExprKind::Call(func, _) = &expr.kind else {
            return;
        };
        let ExprKind::Path(ref qpath) = func.kind else {
            return;
        };
        let res = match qpath {
            QPath::Resolved(_, path) => path.res,
            // Method-style or qualified spellings never match `decode_typed`.
            _ => return,
        };
        if !is_decode_typed_call(res, cx.tcx) {
            return;
        }
        let Some(guard) = enclosing_guard_name(cx.tcx, expr.hir_id) else {
            return;
        };
        match (expected_claims(&guard), decode_target_name(cx, expr)) {
            (Some(expected), Some(actual)) => {
                if actual != expected {
                    cx.emit_span_lint(
                        DECODE_AGREEMENT,
                        expr.span,
                        DiagDecorator(|diag| {
                            diag.primary_message(format!(
                                "`{guard}` decodes `{actual}` but the model assigns `{expected}`"
                            ));
                        }),
                    );
                }
            }
            // Fail closed: a guard with no assigned decode target must not
            // decode, and an unresolvable target cannot be proven correct.
            (expected, actual) => {
                let msg = match (expected, actual) {
                    (None, _) => format!("`{guard}` has no assigned decode target but calls `decode_typed`"),
                    (_, None) => format!("`{guard}` calls `decode_typed` with an unresolvable target type"),
                    _ => return,
                };
                cx.emit_span_lint(
                    DECODE_AGREEMENT,
                    expr.span,
                    DiagDecorator(|diag| {
                        diag.primary_message(msg);
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
