//! Token-typ-agreement lint (AUTH-1/S3): a handler's `x-picasu-auth`
//! `credentials` must agree with the claims types its guards decode.

#![feature(rustc_private)]

extern crate rustc_ast_pretty;
extern crate rustc_errors;
extern crate rustc_hir;

use rustc_errors::DiagDecorator;
use rustc_hir::{Item, ItemKind, Ty, TyKind};
use rustc_lint::{LateContext, LateLintPass, LintContext};

dylint_linting::dylint_library!();

extern crate rustc_lint;
extern crate rustc_session;

#[unsafe(no_mangle)]
pub fn register_lints(sess: &rustc_session::Session, lint_store: &mut rustc_lint::LintStore) {
    dylint_linting::init_config(sess);
    lint_store.register_lints(&[CREDENTIALS_AGREEMENT]);
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(CredentialsAgreement)));
}

rustc_session::declare_lint! {
    pub CREDENTIALS_AGREEMENT,
    Warn,
    "a handler's credentials do not match the claim types its guards decode"
}

rustc_session::declare_lint_pass!(CredentialsAgreement => [CREDENTIALS_AGREEMENT]);

enum GuardKind {
    Typs(Vec<&'static str>),
    None,
    Unknown,
}

fn classify_guard(name: &str) -> GuardKind {
    match name {
        "GuardAdmin" | "GuardUser" => GuardKind::Typs(vec!["session"]),
        "GuardHash" | "GuardHashOriginal" => GuardKind::Typs(vec!["asset"]),
        "GuardRenewHash" | "GuardSnapshot" => GuardKind::Typs(vec!["snapshot"]),
        // GuardPresenter resolves via the share table and falls back to
        // try_jwt_cookie_auth, so it accepts session tokens too.
        "GuardPresenter" => GuardKind::Typs(vec!["session", "share"]),
        "GuardMutate" | "GuardReauth" => GuardKind::None,
        _ => GuardKind::Unknown,
    }
}

fn unwrap_guard_ty<U>(ty: &Ty<'_, U>) -> Option<String> {
    match &ty.kind {
        TyKind::Path(qpath) => {
            let path = match qpath {
                rustc_hir::QPath::Resolved(_, p) => p,
                rustc_hir::QPath::TypeRelative(_, seg) => {
                    return Some(seg.ident.as_str().to_string());
                }
            };
            let last = path.segments.last()?;
            let name = last.ident.as_str().to_string();
            if name == "GuardResult" {
                if let Some(args) = last.args {
                    for arg in args.args {
                        if let rustc_hir::GenericArg::Type(t) = arg {
                            return unwrap_guard_ty(t);
                        }
                    }
                }
                return None;
            }
            Some(name)
        }
        _ => None,
    }
}

fn parse_credentials(tokens: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Some(idx) = tokens.find("credentials") else {
        return out;
    };
    let rest = &tokens[idx..];
    let Some(start) = rest.find('[') else {
        return out;
    };
    let arr = &rest[start..];
    let Some(end) = arr.find(']') else {
        return out;
    };
    let inner = &arr[1..end];
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            let mut s = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                s.push(c);
            }
            out.push(s);
        }
    }
    out
}

impl<'tcx> LateLintPass<'tcx> for CredentialsAgreement {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        let ItemKind::Fn { sig, .. } = &item.kind else {
            return;
        };
        // The utoipa::path attribute is consumed by the proc macro, so recover
        // its text from the original source preceding this function item.
        let sm = cx.tcx.sess.source_map();
        let sf = sm.lookup_source_file(item.span.lo());
        let Some(src) = sf.src.as_ref() else {
            return;
        };
        let start_pos = sf.start_pos.0 as usize;
        let offset = item.span.lo().0 as usize - start_pos;
        if offset > src.len() {
            return;
        }
        let prefix = &src[..offset];
        let Some(attr_start) = prefix.rfind("#[utoipa::path") else {
            return;
        };
        let attr_text = &src[attr_start..offset];
        // Only treat the attribute as belonging to this function if it
        // directly precedes it (no other function in between).
        if attr_text.contains("fn") {
            return;
        }
        let tokens = attr_text;
        if !tokens.contains("x-picasu-auth") {
            return;
        }

        let mut guard_typs: Vec<String> = Vec::new();
        let mut unknown: Vec<String> = Vec::new();
        for ty in sig.decl.inputs {
            let Some(name) = unwrap_guard_ty(ty) else {
                continue;
            };
            if !name.contains("Guard") {
                continue;
            }
            match classify_guard(&name) {
                GuardKind::Typs(ts) => {
                    for t in ts {
                        guard_typs.push(t.to_string());
                    }
                }
                GuardKind::None => {}
                GuardKind::Unknown => unknown.push(name),
            }
        }

        let credentials = parse_credentials(tokens);

        let mut gset = guard_typs.clone();
        gset.sort();
        gset.dedup();
        let mut tset = credentials.clone();
        tset.sort();
        tset.dedup();

        let warn = if !unknown.is_empty() {
            true
        } else {
            gset != tset
        };

        if !warn {
            return;
        }

        let msg = if !unknown.is_empty() {
            format!(
                "unknown guard type(s) {:?}: cannot verify credentials agreement",
                unknown
            )
        } else {
            format!(
                "handler credentials {:?} do not match guard-derived claim types {:?}",
                tset, gset
            )
        };

        cx.emit_span_lint(
            CREDENTIALS_AGREEMENT,
            item.span,
            DiagDecorator(|diag| {
                diag.primary_message(msg);
            }),
        );
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
