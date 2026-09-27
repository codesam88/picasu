//! Rocket request guards, as a handler's signature declares them and its body
//! acts on them.
//!
//! Rocket gives a request guard two shapes, and only one of them is safe on its
//! own. A bare `GuardAuth` parameter is run by Rocket before the handler body, so
//! a request the guard rejects never reaches it. A `GuardResult<T>` or `Option<T>`
//! parameter hands the outcome to the body instead, and there the guard is only as
//! strong as the handler's use of it: `let _ = auth?;` checks the failure and
//! drops the claims, `let _ = auth;` drops both and serves an unauthorized caller.
//! The second shape is a real bug class in this codebase — see
//! `.plan/bug-get-rows-auth-guard-discarded.md` — so a deferred binding is read
//! here and reported when the body does not act on it.
//!
//! # The guard vocabulary is a hand-written assumption
//!
//! The analyzer reads source without compiling it, so a guard is recognised by
//! the last segment of its type path and matched against [`KNOWN_GUARDS`]. That
//! list is a claim about the codebase, not something derived from it, and the
//! claim is only as good as its failure mode: a guard the crate does not
//! recognise is read as no guard at all, so the auth policy reports the route
//! using it as unguarded rather than passing it silently. `KNOWN_GUARDS` is the
//! single place a guard enters the vocabulary, and a test reads the backend's
//! `FromRequest` implementations to hold it to that.

use syn::visit::{self, Visit};
use syn::{Block, Expr, FnArg, ItemFn, Pat, Type};

/// The request-guard types this crate recognises, as the `struct` names the
/// codebase gives them.
///
/// The single place a guard enters the vocabulary: a type added here also needs a
/// [`GuardClass`] mapping to what it enforces, and a test asserts the two agree.
/// Sorted so a diff of the list is readable.
pub const KNOWN_GUARDS: &[&str] = &[
    "GuardAuth",
    "GuardHash",
    "GuardHashOriginal",
    "GuardReadOnlyMode",
    "GuardShare",
    "GuardTimestamp",
    "GuardUpload",
    "TimestampGuardModified",
];

/// The request wrappers that turn a guard parameter into a decision the handler
/// body has to make.
///
/// `Result` and `GuardResult` are the same type here — the backend aliases the
/// latter to the former with the guard's own error — so both names are accepted,
/// and only the first type argument is the guard: `Result<Form<T>, Errors<T>>` is
/// Rocket's form wrapper, not a deferred guard.
const DEFERRED_WRAPPERS: &[&str] = &["Option", "Result", "GuardResult"];

/// What a request guard of this codebase enforces, independent of the type name it
/// is declared under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuardClass {
    /// `GuardAuth`: the admin JWT cookie, checked against the full validation.
    AdminCookie,
    /// `GuardShare`: a share token from the headers or the query, falling back to
    /// the admin cookie.
    Share,
    /// `GuardTimestamp`: a bearer token whose `timestamp` claim must equal the
    /// `timestamp` query parameter.
    Timestamp,
    /// `TimestampGuardModified`: the same bearer token accepted while expired, for
    /// the endpoints whose job is to issue a fresh one.
    TimestampModified,
    /// `GuardHash`: a bearer token whose `hash` claim must equal the serving id in
    /// the URL.
    Hash,
    /// `GuardHashOriginal`: the same check against the token's `asset_id` claim.
    HashOriginal,
    /// `GuardUpload`: a share allowed to upload, falling back to the admin cookie.
    Upload,
    /// `GuardReadOnlyMode`: the server's read-only flag. It never rejects an
    /// unauthenticated caller, so a route behind it is still open unless something
    /// else guards it.
    ReadOnlyMode,
}

impl GuardClass {
    /// Every class, for a caller that has to cover the vocabulary.
    pub const ALL: &'static [Self] = &[
        Self::AdminCookie,
        Self::Share,
        Self::Timestamp,
        Self::TimestampModified,
        Self::Hash,
        Self::HashOriginal,
        Self::Upload,
        Self::ReadOnlyMode,
    ];

    /// The class of the guard type declared under `name`, ignoring any path
    /// qualification so `auth::GuardAuth` and `GuardAuth` are one guard.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "GuardAuth" => Some(Self::AdminCookie),
            "GuardShare" => Some(Self::Share),
            "GuardTimestamp" => Some(Self::Timestamp),
            "TimestampGuardModified" => Some(Self::TimestampModified),
            "GuardHash" => Some(Self::Hash),
            "GuardHashOriginal" => Some(Self::HashOriginal),
            "GuardUpload" => Some(Self::Upload),
            "GuardReadOnlyMode" => Some(Self::ReadOnlyMode),
            _ => None,
        }
    }

    /// The type name a handler declares this guard by, as a diagnostic should
    /// spell it: the developer has to type this name to satisfy the policy.
    #[must_use]
    pub fn guard_name(self) -> &'static str {
        match self {
            Self::AdminCookie => "GuardAuth",
            Self::Share => "GuardShare",
            Self::Timestamp => "GuardTimestamp",
            Self::TimestampModified => "TimestampGuardModified",
            Self::Hash => "GuardHash",
            Self::HashOriginal => "GuardHashOriginal",
            Self::Upload => "GuardUpload",
            Self::ReadOnlyMode => "GuardReadOnlyMode",
        }
    }

    /// Whether a request this guard rejects is answered `401 Unauthorized`.
    ///
    /// False only for [`Self::ReadOnlyMode`], which answers `405`. The distinction
    /// is why the policy names guard classes instead of counting guards: a
    /// read-only route is a write route that is additionally closed while the
    /// server is read-only, and it says nothing about authentication.
    #[must_use]
    pub fn rejects_with_unauthorized(self) -> bool {
        !matches!(self, Self::ReadOnlyMode)
    }
}

/// One request guard read from a handler's parameter list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardBinding {
    /// What the parameter's guard enforces.
    pub class: GuardClass,
    /// The name the handler binds the guard to. A direct guard's name is part of
    /// the same information — `_auth` says the body never reads it.
    pub parameter: String,
    /// 1-based line of the parameter in the signature, so a finding about the
    /// binding points at the parameter rather than at the function.
    pub line: usize,
    /// Whether the guard is actually enforced.
    pub enforcement: Enforcement,
}

/// How a guard binding takes effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enforcement {
    /// Rocket runs the guard before the body, so a rejected request never reaches
    /// it and the body cannot weaken it.
    ByRocket,
    /// The guard arrives as a `Result`/`Option` and the body acts on the failure:
    /// propagated with `?`, returned, matched, or inspected.
    ByHandler,
    /// A deferred binding the body does not act on, which serves the request as
    /// though the guard had passed.
    Discarded(Discard),
}

/// How a deferred guard binding loses its failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Discard {
    /// Read and thrown away by a wildcard binding, `let _ = auth;`. The value is
    /// touched, so the code reads as if the failure had been checked, and the
    /// difference from `let _ = auth?;` is a single character.
    Dropped,
    /// Never mentioned in the body at all.
    Unread,
}

/// The guard parameters of one handler function, in signature order.
pub(crate) fn bindings(item: &ItemFn) -> Vec<GuardBinding> {
    item.sig
        .inputs
        .iter()
        .filter_map(|input| binding(input, &item.block))
        .collect()
}

/// The guard one parameter declares, or `None` for a receiver or a non-guard.
fn binding(input: &FnArg, body: &Block) -> Option<GuardBinding> {
    let FnArg::Typed(typed) = input else {
        return None;
    };
    let Pat::Ident(name) = typed.pat.as_ref() else {
        return None;
    };

    let (class, deferred) = classify(&typed.ty)?;
    let parameter = name.ident.to_string();
    let enforcement = if deferred {
        let mut use_of_binding = BindingUse::of(&parameter);
        use_of_binding.visit_block(body);
        use_of_binding.enforcement()
    } else {
        Enforcement::ByRocket
    };

    Some(GuardBinding {
        class,
        parameter,
        line: line_of(&name.ident),
        enforcement,
    })
}

/// The guard class a parameter type names, and whether Rocket hands the decision
/// to the body.
fn classify(ty: &Type) -> Option<(GuardClass, bool)> {
    let (ty, deferred) = match unwrap_reference(ty) {
        Type::Path(path) => match deferred_argument(path) {
            Some(inner) => (unwrap_reference(inner), true),
            None => (unwrap_reference(ty), false),
        },
        other => (other, false),
    };

    let Type::Path(path) = ty else {
        return None;
    };
    let name = path.path.segments.last()?.ident.to_string();
    GuardClass::from_name(&name).map(|class| (class, deferred))
}

/// The type a reference points at, so `&GuardAuth` is the guard `GuardAuth`.
fn unwrap_reference(ty: &Type) -> &Type {
    match ty {
        Type::Reference(reference) => unwrap_reference(&reference.elem),
        Type::Paren(paren) => unwrap_reference(&paren.elem),
        other => other,
    }
}

/// The guarded type inside a `GuardResult<T>`/`Option<T>` parameter, if that is
/// what the parameter is.
///
/// `Result` takes its first type argument, because the second is the error — the
/// shape Rocket's form wrapper uses, where the first argument is a `Form` and
/// therefore not a guard at all.
fn deferred_argument(path: &syn::TypePath) -> Option<&Type> {
    let name = path.path.segments.last()?.ident.to_string();
    if !DEFERRED_WRAPPERS.contains(&name.as_str()) {
        return None;
    }
    let arguments = match &path.path.segments.last()?.arguments {
        syn::PathArguments::AngleBracketed(arguments) => &arguments.args,
        _ => return None,
    };

    arguments.iter().find_map(|argument| match argument {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    })
}

/// What a handler body does with one binding: whether it observes the failure,
/// and whether it drops it outright.
struct BindingUse<'a> {
    parameter: &'a str,
    observed: bool,
    dropped: bool,
}

impl<'a> BindingUse<'a> {
    fn of(parameter: &'a str) -> Self {
        Self {
            parameter,
            observed: false,
            dropped: false,
        }
    }

    /// The shape to report, from the two facts the walk collected.
    fn enforcement(&self) -> Enforcement {
        if self.observed {
            Enforcement::ByHandler
        } else if self.dropped {
            Enforcement::Discarded(Discard::Dropped)
        } else {
            Enforcement::Discarded(Discard::Unread)
        }
    }

    /// Whether an expression reads this binding, following the method calls and
    /// borrows on top of it.
    ///
    /// `auth`, `auth.as_ref()` and `&auth` are the same read, which is what makes
    /// `auth.map(..)?` an observation of `auth`'s failure. A multi-segment path is
    /// not a read of the binding: `self.auth` shares its last segment with a
    /// parameter called `auth` and is a different value.
    fn reads(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Path(path) => {
                path.qself.is_none()
                    && path.path.segments.len() == 1
                    && path.path.segments[0].ident == self.parameter
            }
            Expr::Reference(reference) => self.reads(&reference.expr),
            Expr::Paren(paren) => self.reads(&paren.expr),
            Expr::Field(field) => self.reads(&field.base),
            Expr::MethodCall(call) => self.reads(&call.receiver),
            Expr::Try(probe) => self.reads(&probe.expr),
            _ => false,
        }
    }
}

impl<'ast> Visit<'ast> for BindingUse<'_> {
    fn visit_stmt(&mut self, stmt: &'ast syn::Stmt) {
        match stmt {
            // A wildcard binding is the shape a dropped guard takes, and it is
            // only recognisable as a statement: `let _ = auth;`. An init carrying
            // its own `?` is a propagation rather than a drop, and is read as one.
            syn::Stmt::Local(local) => {
                if matches!(&local.pat, Pat::Wild(_))
                    && let Some(init) = &local.init
                    && !matches!(&*init.expr, Expr::Try(_))
                    && self.reads(&init.expr)
                {
                    self.dropped = true;
                }
            }
            // A semicolon-less expression statement is a block's value: `auth` at
            // the end of a body forwards the failure to the caller instead of
            // dropping it inside the handler.
            syn::Stmt::Expr(expr, None) if self.reads(expr) => self.observed = true,
            _ => {}
        }
        visit::visit_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        if self.observed {
            return;
        }
        match expr {
            // `auth?` propagates the failure out of the handler, and `return auth`
            // hands it to the caller; both are the guard being acted on.
            Expr::Try(probe) if self.reads(&probe.expr) => self.observed = true,
            Expr::Return(ret) if ret.expr.as_ref().is_some_and(|expr| self.reads(expr)) => {
                self.observed = true;
            }
            // `match auth { .. }` and `let Ok(claims) = auth` inspect the outcome.
            Expr::Match(branch) if self.reads(&branch.expr) => self.observed = true,
            Expr::Let(binding) if self.reads(&binding.expr) => self.observed = true,
            // `auth.is_ok()`, `auth.map_err(..)` and friends: reading the `Result`
            // through a method is only an observation when the method is one of the
            // ways of looking at whether it failed.
            Expr::MethodCall(call)
                if self.reads(&call.receiver)
                    && OBSERVING_METHODS.contains(&call.method.to_string().as_str()) =>
            {
                self.observed = true;
            }
            _ => {}
        }
        if !self.observed {
            visit::visit_expr(self, expr);
        }
    }
}

/// The `Result`/`Option` methods that look at whether the value failed.
///
/// Deliberately short: a guard is enforced when the handler can see its failure.
/// A method that only touches the value — `auth.as_ref()` — is not one of these,
/// and reading a field of the `Result` (`auth.claims`) is not either, which is
/// precisely the mistake the wildcard-binding rule exists to catch.
const OBSERVING_METHODS: &[&str] = &[
    "and_then",
    "err",
    "expect",
    "is_err",
    "is_ok",
    "map",
    "map_err",
    "map_or",
    "ok",
    "unwrap_or",
    "unwrap_or_default",
    "unwrap_or_else",
];

/// The 1-based line a name is on, or 1 when the input carried no span.
fn line_of(ident: &syn::Ident) -> usize {
    let line = ident.span().start().line;
    if line > 0 { line } else { 1 }
}
