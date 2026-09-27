//! The authentication policy of the documented API, and the checks that hold the
//! source to it.
//!
//! Everything else in this crate compares the *shape* of the contract: a handler
//! against its annotation, an operation against the document. Nothing here looks
//! at who may call an operation, which is the one property of a public API that a
//! consumer cannot read off the spec — an operation that documents a 401 and
//! serves an unauthorized caller looks exactly like one that enforces the guard.
//! That gap is not hypothetical: `GET /get/get-rows` and `GET /get/get-scroll-bar`
//! discarded their guard result with `let _ = auth;` and answered 200 to an
//! anonymous caller (`.plan/bug-get-rows-auth-guard-discarded.md`). The document
//! was correct throughout, and so was the handler's signature.
//!
//! # The policy is a table of every operation, not a list of exceptions
//!
//! [`AUTH_POLICY`] has one entry per documented operation, and an operation with
//! no entry is a finding. The alternative — listing the public operations and
//! treating everything else as protected — was rejected: it makes the set of
//! protected operations implicit, so deleting the guard from a protected handler
//! leaves the policy untouched and the route unguarded, which is the failure this
//! module exists to catch. An exhaustive table turns the same deletion into a
//! mismatch between the policy and the handler, and it makes every new operation
//! a reviewed decision: the author has to say which guard it wants, or that it is
//! deliberately open. The cost is a line per operation when one is added, which is
//! the price of a reviewed fact rather than an inherited default.
//!
//! # Keyed by `operationId`, not by path
//!
//! An entry is keyed by the operation's `operationId`, not by `(method, path)`.
//! Authentication does not change when a route moves, and a path key would make
//! every rename look like a new operation: the old entry would be reported stale
//! and the new operation unlisted, so a rename nobody intended as a security
//! change would have to be re-reviewed as one. The `operationId` is the stable
//! name a generated client calls the operation by, which is the same reason it is
//! the name worth binding a policy to — and a handler rename, which does change
//! it, fails loudly as a stale entry instead of silently inheriting a guard.
//!
//! # What a rule can and cannot say
//!
//! A rule names the guard classes the handler must declare, and that is the whole
//! of it: whether those guards are actually *used* as the policy intends is the
//! job of the handler, and the one place a check could overreach — a
//! `GuardShare` route whose body ignores the share claims is legitimate, since
//! several of them only want the authentication that falls out of it. Security is
//! therefore not inferred from subject tags: a tag is a documentation grouping,
//! and `pages` on a route says nothing about whether the SPA behind it is public.

use std::collections::{BTreeMap, BTreeSet};

use crate::contract::{
    Declaration, HandlerKey, Registration, SpecOperation, declared_operation, identity,
    is_excluded, read_sources, verb,
};
use crate::finding::Finding;
use crate::guards::GuardClass;
use crate::handlers::{Handler, HttpMethod};
use crate::modules::SourceUnit;

/// What a public operation's `401` means when no request guard produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unauthenticated {
    /// The operation is open and cannot answer `401`. The default for a public
    /// operation: no guard, and no way to reject a caller.
    Never,
    /// The handler checks the credentials itself and answers `401` on failure.
    /// How a caller obtains the token the other guards expect, so being open is
    /// the operation's purpose rather than an omission.
    CheckedByHandler,
    /// A public page whose own response body *is* a 401 — the landing page a
    /// guarded route redirects to. It answers 401 without authenticating anyone,
    /// which is why it is named rather than exempted.
    LandingPage,
}

/// What the policy requires of one documented operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthRule {
    /// The operation's `operationId`, the key the policy is indexed by.
    pub operation_id: &'static str,
    /// The guard classes the handler must declare, as a set.
    ///
    /// Empty means the operation is public. A non-empty set is compared for
    /// equality, not containment: a guard added to a public page route, or
    /// swapped for a weaker one, changes what the route accepts and has to be a
    /// reviewed policy change.
    pub guards: &'static [GuardClass],
    /// Why a guardless operation still documents a `401`.
    pub unauthenticated: Unauthenticated,
}

impl AuthRule {
    /// An operation behind exactly these guards.
    #[must_use]
    pub const fn guarded(operation_id: &'static str, guards: &'static [GuardClass]) -> Self {
        Self {
            operation_id,
            guards,
            unauthenticated: Unauthenticated::Never,
        }
    }

    /// An operation reachable without credentials and unable to answer `401`.
    #[must_use]
    pub const fn public(operation_id: &'static str) -> Self {
        Self {
            operation_id,
            guards: &[],
            unauthenticated: Unauthenticated::Never,
        }
    }

    /// Whether the operation can answer `401` at all.
    ///
    /// True when one of its guards rejects with 401, or when the handler rejects
    /// on its own. [`GuardClass::rejects_with_unauthorized`] is what keeps a
    /// `GuardReadOnlyMode` route — which answers 405 — from being counted as
    /// documented authentication.
    #[must_use]
    pub fn documents_unauthorized(&self) -> bool {
        self.unauthenticated != Unauthenticated::Never
            || self
                .guards
                .iter()
                .any(|class| class.rejects_with_unauthorized())
    }
}

/// The authentication policy of the documented API: one entry per operation.
///
/// Sorted by `operationId` so a diff of the table is reviewable, and exhaustive
/// so that both directions fail — an operation with no entry is reported, and an
/// entry no operation answers to is reported too.
///
/// The 22 `pages` operations are public: they serve the SPA shell, and the data
/// behind them is guarded by the API operations. The one guardless operation that
/// is not a page is the login endpoint, which is where a caller obtains the token
/// the other guards check.
pub const AUTH_POLICY: &[AuthRule] = &[
    // ── Public: the SPA shell, plus the static assets it loads ────────────────
    AuthRule::public("redirect_to_photo"),
    AuthRule::public("albums"),
    AuthRule::public("albums_view"),
    AuthRule::public("album_page"),
    AuthRule::public("config"),
    AuthRule::public("favicon"),
    AuthRule::public("links"),
    AuthRule::public("login"),
    AuthRule::public("redirect_to_login"),
    AuthRule::public("sregister_sw"),
    AuthRule::public("service_worker"),
    AuthRule::public("setting"),
    AuthRule::public("share"),
    AuthRule::public("spa_fallback"),
    AuthRule::public("tags"),
    AuthRule::public("timeline"),
    AuthRule::public("timeline_view"),
    AuthRule::public("trashed"),
    AuthRule::public("trashed_view"),
    AuthRule::public("videos"),
    AuthRule::public("videos_view"),
    // The landing page a guarded route redirects to: its 401 is the response body.
    AuthRule {
        operation_id: "unauthorized",
        guards: &[],
        unauthenticated: Unauthenticated::LandingPage,
    },
    // The password comparison is the guard: it is what mints the admin token.
    AuthRule {
        operation_id: "authenticate",
        guards: &[],
        unauthenticated: Unauthenticated::CheckedByHandler,
    },
    // ── Grid and list data: a timestamp token from the session ───────────────
    AuthRule::guarded("get_data", &[GuardClass::Timestamp]),
    AuthRule::guarded("get_rows", &[GuardClass::Timestamp]),
    AuthRule::guarded("get_scroll_bar", &[GuardClass::Timestamp]),
    AuthRule::guarded("get_tags", &[GuardClass::AdminCookie]),
    AuthRule::guarded("get_albums", &[GuardClass::AdminCookie]),
    AuthRule::guarded("get_album_index_status", &[GuardClass::AdminCookie]),
    AuthRule::guarded("get_metadata", &[GuardClass::Timestamp]),
    AuthRule::guarded("get_export", &[GuardClass::AdminCookie]),
    AuthRule::guarded("prefetch", &[GuardClass::Share]),
    AuthRule::guarded("get_config_handler", &[GuardClass::Share]),
    AuthRule::guarded("export_config_handler", &[GuardClass::AdminCookie]),
    AuthRule::guarded("get_fs_completion", &[GuardClass::AdminCookie]),
    // ── Serving: a share or an admin cookie, plus a token bound to the URL ────
    AuthRule::guarded("compressed_file", &[GuardClass::Share, GuardClass::Hash]),
    AuthRule::guarded(
        "imported_file",
        &[GuardClass::Share, GuardClass::HashOriginal],
    ),
    // ── Token renewal: accepts the token it is asked to replace ───────────────
    AuthRule::guarded("renew_timestamp_token", &[GuardClass::Share]),
    AuthRule::guarded("renew_hash_token", &[GuardClass::TimestampModified]),
    // ── Configuration: the admin cookie, and read-only closes writes ──────────
    AuthRule::guarded("import_config_handler", &[GuardClass::AdminCookie]),
    AuthRule::guarded(
        "update_config_handler",
        &[GuardClass::AdminCookie, GuardClass::ReadOnlyMode],
    ),
    AuthRule::guarded(
        "update_password_handler",
        &[GuardClass::AdminCookie, GuardClass::ReadOnlyMode],
    ),
    // ── Indexing ─────────────────────────────────────────────────────────────
    AuthRule::guarded(
        "index_album_handler",
        &[GuardClass::AdminCookie, GuardClass::ReadOnlyMode],
    ),
    AuthRule::guarded(
        "index_image_handler",
        &[GuardClass::AdminCookie, GuardClass::ReadOnlyMode],
    ),
    AuthRule::guarded("cancel_album_index_handler", &[GuardClass::AdminCookie]),
    AuthRule::guarded(
        "rebuild_handler",
        &[GuardClass::AdminCookie, GuardClass::ReadOnlyMode],
    ),
    // ── Albums and shares ────────────────────────────────────────────────────
    AuthRule::guarded(
        "create_dir_album",
        &[GuardClass::AdminCookie, GuardClass::ReadOnlyMode],
    ),
    AuthRule::guarded(
        "create_share",
        &[GuardClass::AdminCookie, GuardClass::ReadOnlyMode],
    ),
    AuthRule::guarded(
        "assign_album",
        &[GuardClass::AdminCookie, GuardClass::ReadOnlyMode],
    ),
    AuthRule::guarded(
        "set_album_cover",
        &[GuardClass::AdminCookie, GuardClass::ReadOnlyMode],
    ),
    AuthRule::guarded(
        "set_album_title",
        &[GuardClass::Share, GuardClass::ReadOnlyMode],
    ),
    AuthRule::guarded(
        "set_user_defined_description",
        &[GuardClass::Share, GuardClass::ReadOnlyMode],
    ),
    AuthRule::guarded(
        "edit_share",
        &[GuardClass::AdminCookie, GuardClass::ReadOnlyMode],
    ),
    AuthRule::guarded(
        "delete_share",
        &[GuardClass::AdminCookie, GuardClass::ReadOnlyMode],
    ),
    // ── Per-asset metadata ───────────────────────────────────────────────────
    AuthRule::guarded(
        "edit_flags",
        &[GuardClass::AdminCookie, GuardClass::ReadOnlyMode],
    ),
    AuthRule::guarded(
        "edit_rating",
        &[GuardClass::AdminCookie, GuardClass::ReadOnlyMode],
    ),
    AuthRule::guarded(
        "edit_tag",
        &[GuardClass::AdminCookie, GuardClass::ReadOnlyMode],
    ),
    AuthRule::guarded(
        "rotate_image",
        &[GuardClass::AdminCookie, GuardClass::ReadOnlyMode],
    ),
    AuthRule::guarded(
        "regenerate_thumbnail_with_frame",
        &[GuardClass::AdminCookie, GuardClass::ReadOnlyMode],
    ),
    AuthRule::guarded(
        "delete_data",
        &[GuardClass::AdminCookie, GuardClass::ReadOnlyMode],
    ),
    // ── Upload ───────────────────────────────────────────────────────────────
    AuthRule::guarded("upload", &[GuardClass::Upload, GuardClass::ReadOnlyMode]),
];

/// Every way the observed authentication of the source disagrees with the policy.
///
/// Four rules, one per direction they can fail in:
///
/// | Finding | Drift it catches |
/// | --- | --- |
/// | the policy requires a guard the handler does not declare | a guard removed from a protected handler |
/// | the handler declares a guard the policy does not list | a route closed (or opened) without a policy change |
/// | the operation is in no policy entry | a new operation nobody classified |
/// | the policy names an operation the document does not declare | a removed operation whose entry was left behind |
/// | the operation documents no 401 / documents an unexpected 401 | a guard whose rejection the contract does not describe |
///
/// A deferred guard the handler drops is reported by the scan instead, from
/// [`crate::scan_handlers`]: it is a defect in the handler rather than a
/// disagreement about the policy, and it holds for a route the policy does not
/// describe.
///
/// `units` and `excluded_prefixes` mean what they mean in [`crate::check_contract`]:
/// the excluded operations are outside the compared contract in both directions,
/// so a test-only prefix does not have to be classified here. An operation the
/// source does not declare is left to that check, which reports it; this one has
/// no handler to observe.
///
/// Findings are sorted by file, line and message, so two runs over the same inputs
/// report the same things in the same order.
#[must_use]
pub fn check_auth(
    units: &[SourceUnit<'_>],
    spec_label: &str,
    spec: &[SpecOperation<'_>],
    excluded_prefixes: &[&str],
    policy: &[AuthRule],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let (declarations, mut registrations) = read_sources(units, &mut findings);
    registrations.sort();

    let compared = Compared::new(spec_label, spec, excluded_prefixes, policy);
    compared.check_coverage(&mut findings);
    compared.check_handlers(
        &registrations,
        &declarations,
        excluded_prefixes,
        &mut findings,
    );

    findings.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.message.cmp(&b.message))
    });
    findings
}
/// A `(method, path)` pair, the identity an operation is compared by.
type Operation = (HttpMethod, String);

/// The policy and the document, indexed the two ways the checks look them up.
///
/// A handler knows its operation by route and a policy entry by name, so the same
/// document has to be reachable both ways; building the two indexes once is what
/// keeps the halves of the gate resolving an operation to the same rule.
struct Compared<'a> {
    spec_label: &'a str,
    rules: BTreeMap<&'a str, &'a AuthRule>,
    documented: Vec<&'a SpecOperation<'a>>,
    by_operation: BTreeMap<Operation, &'a SpecOperation<'a>>,
}

impl<'a> Compared<'a> {
    fn new(
        spec_label: &'a str,
        spec: &'a [SpecOperation<'a>],
        excluded_prefixes: &[&str],
        policy: &'a [AuthRule],
    ) -> Self {
        let rules: BTreeMap<&str, &AuthRule> = policy
            .iter()
            .map(|rule| (rule.operation_id, rule))
            .collect();
        let documented: Vec<&SpecOperation<'_>> = spec
            .iter()
            .filter(|operation| !is_excluded(operation.path, excluded_prefixes))
            .collect();
        let by_operation = documented
            .iter()
            .map(|operation| ((operation.method, operation.path.to_string()), *operation))
            .collect();

        Self {
            spec_label,
            rules,
            documented,
            by_operation,
        }
    }

    /// Every documented operation is classified, and every classified operation
    /// exists.
    ///
    /// Both directions, because either one alone is satisfied by a policy table
    /// that has drifted away from the API: a table that classifies only the
    /// operations a reviewer remembered leaves the rest open by default, and a
    /// table that outlives its operations reads as a set of deliberate exceptions
    /// rather than the mistakes it is.
    fn check_coverage(&self, findings: &mut Vec<Finding>) {
        let mut classified: BTreeSet<&str> = BTreeSet::new();
        for operation in &self.documented {
            match operation
                .operation_id
                .and_then(|id| self.rules.get(id).copied())
            {
                Some(_) => {
                    if let Some(id) = operation.operation_id {
                        classified.insert(id);
                    }
                }
                None => findings.push(Finding {
                    file: self.spec_label.to_string(),
                    line: None,
                    message: match operation.operation_id {
                        Some(id) => format!(
                            "{} {} (operationId `{id}`) is in no auth policy entry — \
                             add one naming its guards, or one marking it a public \
                             exception",
                            verb(operation.method),
                            operation.path
                        ),
                        None => format!(
                            "{} {} declares no operationId, so the auth policy cannot \
                             be keyed by it",
                            verb(operation.method),
                            operation.path
                        ),
                    },
                }),
            }
        }

        for id in self.rules.keys() {
            if !classified.contains(id) {
                findings.push(Finding {
                    file: self.spec_label.to_string(),
                    line: None,
                    message: format!(
                        "auth policy entry `{id}` names an operation the document \
                         does not declare — remove the stale entry"
                    ),
                });
            }
        }
    }

    /// Every registered handler agrees with the policy for the operation it
    /// declares.
    fn check_handlers(
        &self,
        registrations: &[Registration],
        declarations: &BTreeMap<HandlerKey, Declaration>,
        excluded_prefixes: &[&str],
        findings: &mut Vec<Finding>,
    ) {
        let mut seen: BTreeSet<&HandlerKey> = BTreeSet::new();
        for registration in registrations {
            // The first registration of a handler is the one that declares its
            // contract; the rest are the duplicates `check_contract` reports, and
            // re-comparing the same handler would report its drift twice.
            if !seen.insert(&registration.key) {
                continue;
            }
            let Some(declaration) = declarations.get(&registration.key) else {
                // A registered handler no scanned file declares has no signature to
                // observe; `check_contract` names the missing declaration.
                continue;
            };
            let Some(operation) = declared_operation(&declaration.handler) else {
                continue;
            };
            if is_excluded(&operation.path, excluded_prefixes) {
                continue;
            }
            // Matched through the committed document rather than through the
            // annotation, because the policy describes the contract as published
            // and an operation's identity there is its `operationId`.
            let Some(documented) = self
                .by_operation
                .get(&(operation.method, operation.path.clone()))
            else {
                continue;
            };
            let Some(rule) = documented
                .operation_id
                .and_then(|id| self.rules.get(id).copied())
            else {
                // An operation in no policy entry is `check_coverage`'s finding,
                // and saying it is also unguarded would restate it from a second
                // angle.
                continue;
            };

            check_guards(
                &declaration.label,
                &identity(&registration.key),
                &declaration.handler,
                rule,
                findings,
            );
            check_documented_rejection(self.spec_label, documented, rule, findings);
        }
    }
}

/// The handler declares exactly the guards the policy lists for it.
///
/// Compared as a set rather than by containment in either direction: a missing
/// guard is the drift this exists to catch, and an extra one closes a route
/// nobody reviewed closing.
fn check_guards(
    label: &str,
    identity: &str,
    handler: &Handler,
    rule: &AuthRule,
    findings: &mut Vec<Finding>,
) {
    let observed: BTreeSet<GuardClass> = handler.guards.iter().map(|b| b.class).collect();
    let required: BTreeSet<GuardClass> = rule.guards.iter().copied().collect();
    if observed == required {
        return;
    }

    let drift = if required.is_empty() {
        format!(
            "{identity}: the auth policy marks this operation public but the handler \
             declares {}",
            render(&observed)
        )
    } else if observed.is_empty() {
        format!(
            "{identity}: the auth policy requires {} but the handler declares no request guard",
            render(&required)
        )
    } else {
        format!(
            "{identity}: the auth policy requires {} but the handler declares {}",
            render(&required),
            render(&observed)
        )
    };
    findings.push(Finding::on_line(label, handler.line, drift));
}

/// The operation documents the rejection its policy says it can answer.
fn check_documented_rejection(
    spec_label: &str,
    operation: &SpecOperation<'_>,
    rule: &AuthRule,
    findings: &mut Vec<Finding>,
) {
    let expected = rule.documents_unauthorized();
    let documented = operation.documents_rejection();
    if expected == documented {
        return;
    }

    findings.push(Finding {
        file: spec_label.to_string(),
        line: None,
        message: if expected {
            format!(
                "{} {} is guarded but documents no 401 response — add \
                 `(status = 401, response = Unauthorized)` to its #[utoipa::path]",
                verb(operation.method),
                operation.path
            )
        } else {
            format!(
                "{} {} is a public operation but documents a 401 — add an auth policy \
                 entry saying who it authenticates, or drop the response",
                verb(operation.method),
                operation.path
            )
        },
    });
}

/// A set of guard classes as a diagnostic spells it, in a stable order.
fn render(classes: &BTreeSet<GuardClass>) -> String {
    if classes.len() == 1 {
        return classes
            .iter()
            .next()
            .map_or_else(String::new, |class| class.guard_name().to_string());
    }
    let names: Vec<&str> = classes.iter().map(|class| class.guard_name()).collect();
    format!("[{}]", names.join(", "))
}
