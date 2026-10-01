---
status: open
type: feature
priority: high
area: backend
---

## Notes

The concrete assertions this repository wants on every `#[utoipa::path]`
annotation, replacing the source/spec rule set that was deleted with the
`openapi-sanity` crate. Drafted 2026-09-28 after `rocket_extras` (the annotation
no longer restates its route), `#[utoipauto]` (no file list) and
`--check-openapi` (route-set parity proven at runtime) landed.

### The filter

A check belongs here only if **both** hold:

1. It reads a fact that exists only in source — not in the document, not in
   Rocket's mount table.
2. Nothing else already enforces it: not rustc, not utoipa's derivation, not
   `openapi-json-match`, not `--check-openapi`, not the committed-artifact diff.

### What the deleted checker covered, and what covers it now

| Deleted rule                                                                           | Now enforced by                                                                            |
| -------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| registered in `routes![]` without an annotation                                        | rustc (the derive references `__path_<fn>`) plus `--check-openapi` (mounted, undocumented) |
| source↔document, both directions                                                       | `openapi-json-match`, `openapi-routes-match`                                               |
| route verb and path vs annotation                                                      | `rocket_extras`: the route's spelling wins, so there is nothing to compare                 |
| route segments/query vs declared parameter names                                       | `rocket_extras` derives them; the residue is B1/B2                                         |
| tag policy, `operationId` uniqueness, `$ref` and orphan schemas, missing `operationId` | document-shaped; nothing enforces them, which is a deliberate gap recorded below           |

### Guard tokens — the authentication surface is currently undocumented

`backend/openapi.json` declares **no `securitySchemes` and no per-operation
`security`**: 61 operations, zero authentication requirements. A consumer of the
spec learns that an operation may answer `401` and nothing else. The one artifact
that used to record it — `AUTH_POLICY`, 61 entries keyed by `operationId` — was
deleted with the crate, and its guard rule caught a real bug (`84f29aa5`, the
discarded `GuardResult`). So the checker's guard rules are not a regression
against a covered area; they are the replacement for the only record we had.

Seven guard classes exist today, and they are three different things:

- **Authentication** (they reject a caller who cannot prove who they are, and
  belong in the document): `GuardAuth`, `GuardTimestamp`, `GuardHash`,
  `GuardHashOriginal`, `GuardShare`, `GuardUpload`.
- **Mode restriction** (they constrain what this build may do, and are not
  authentication): `GuardReadOnlyMode`.
- **Explicit re-authentication** (decided 2026-09-28, not yet implemented): a
  valid token is not enough, the caller must present the account password again.
  It becomes its own guard rather than a field in a request body, because two
  operations need it and the requirement belongs to the operation rather than to
  one handler's payload: disabling read-only mode
  (`.plan/bug-readonly-lockout.md`) and changing the user password. Because the
  requirement is _in addition to_ the token, these operations declare two
  security requirements — see the open question below on whether `security` can
  carry that.

The design, in two parts:

- **Standard, consumer-visible.** Register one security scheme in the document
  and declare `security(("bearer_auth" = []))` on every operation whose route
  carries an authentication guard. `securitySchemes` does not exist in the
  document yet and must be added to `ApiDoc`'s `components(...)`.
- **Repo-specific class, only if the review wants it in the document.** An
  extension in the shape already used for features —
  `extensions(("x-picasu-constraints" = json!(["auth", "read_only_mode"])))` —
  would say _which kind_, which `security` cannot express. Note the name: the
  list carries constraints, not only authentication, so `x-picasu-auth` would be
  a slight lie on the mode and re-authentication routes.
- **Explicit re-authentication has no honest `security` scheme.** OpenAPI's
  vocabulary is http/bearer, apiKey, oauth2, openIdConnect, mutualTLS; "present
  the account password again" is none of those, and an `apiKey` scheme would tell
  a generated client to send a header we do not define. **Open question for the
  user:** either declare a second scheme anyway (`password_reauth`, as
  `apiKey in header`) so tooling can prompt for it, or keep it out of
  `security` and carry it in the extension only. The recommendation is the
  extension, because a wrong scheme is worse than an absent one — a client that
  believes it may send a header we ignore is a client that believes it is
  authenticated when it is not.

Public operations (`renew_hash_token`, `renew_timestamp_token`, the login and
page routes) carry no authentication guard and must declare no `security`; the
rules below are written so that neither direction fires on them.

### A — the annotation's shape

Per annotation, no cross-file knowledge, no false positives found against the
current tree (63 annotations).

| #   | assertion                                             | why nothing else enforces it                                                                      |
| --- | ----------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| A1  | no `path = "..."` and no bare verb                    | the convention exists only in review; nothing rejects a re-introduced duplicate                   |
| A2  | `responses(...)` present and non-empty                | utoipa invents no response, so the omission is silent                                             |
| A3  | exactly one `tag`, from the closed vocabulary of nine | the vocabulary is documented in `docs/openapi-generator.md`; its checker was deleted              |
| A4  | the handler carries a doc comment                     | `summary`/`description` are derived from it, so the omission is invisible except in the reference |
| A5  | the summary is one line                               | a multi-line summary splits the generated reference's headings — measured, not stylistic          |
| A6  | no hand-set `operation_id`                            | utoipa derives it from the function name; a hand-set one is the only name nothing compares        |

### B — what the annotation declares against what the route already says

The invariant: **the route wins, and anything the annotation restates must agree
with it.** This is the residue `rocket_extras` leaves, and it is source-only.

| #   | assertion                                                                                                                         | why nothing else enforces it                                                            |
| --- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| B1  | every declared `params(...)` name corresponds to a route segment (`<name>`, `in: path`) or query binding (`?<name>`, `in: query`) | utoipa merges declared parameters into the derived document without checking they exist |
| B2  | a declared parameter's `required` equals `!argument_is_option` for the handler argument of that name                              | same                                                                                    |
| B3  | a declared `request_body` schema equals the type the route's `data = "..."` binds                                                 | utoipa takes the declared schema and never compares it to the route's binding           |

### C — the handler body

| #   | assertion                                                                                                                                                                                          | calibration                                                                                                                                                                                                                                                                                                                                                                          |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| C1  | **a `GuardResult<…>` argument must have its rejection propagated**: the binding appears as `ident?`, is matched, or is forwarded. `let _ = ident;` and a binding absent from the body are findings | the tree has 52 `GuardResult<…>` bindings, all consumed as `let _ = ident?;`, so the conforming idiom is `let _ = ident?;` and the rule fires on the `84f29aa5` shape (`GuardResult<GuardTimestamp>` dropped without `?`). Zero findings today; it earns its place as a regression guard and must be proved by a mutation test (the deleted suite's `dropping_a_guard_result_fails`) |
| C1b | **a plain `Guard…` argument needs nothing in the body**, and must not be flagged                                                                                                                   | Rocket runs the guard during request handling and short-circuits on failure, so the handler legitimately never touches the value. Nine `_auth: GuardAuth` bindings across seven handlers are never touched. A rule that treated every guard type alike would report every one of those correct handlers as a finding                                                                 |
| C2  | every `pub fn generate_*_routes()` is mounted in `router/builder.rs`                                                                                                                               | not coverage — `--check-openapi` catches an unmounted group — but the gate's message ("documented operation, no route mounts") sends the reader to the document instead of to `builder.rs`                                                                                                                                                                                           |

#### C's known limitations, recorded so they are not rediscovered

- **`GuardResult<…>` is a crate alias** for `Result<T, AppError>`, and C1 matches
  the alias by name. A handler that spells the type out longhand would be
  invisible to it. Nothing does today; the same alias knowledge matters more for
  B2, which compares declared parameter types.
- **Guard classification is by type name, and that is enough only for C1/C1b.**
  `renew_hash_token` binds `TimestampGuardModified`, a plain Rocket guard that
  does not match the `Guard` prefix — harmless here, because these two rules care
  about the fallible alias and not about the class. The D-series cannot: it needs
  to know which guard _class_ a parameter is. That needs a real signal on the
  guard types (a `const fn` such as `is_authentication()`, or reading the class off
  the route attribute), not a name prefix. Recorded here as a design requirement
  for D.
- **A one-hop rebinding is reported, not accepted.** `let x = auth; … x?;`
  propagates in the author's intent and is a finding today, as is a `move` into a
  closure that does not itself propagate. Neither occurs in the tree; the safe
  direction is to report and let a human decide, but both should be pinned by
  fixtures so the behaviour is a decision rather than an accident.

### D — the guard and `security` rules

| #   | assertion                                                                                                                                                | direction that matters                                                                                                                      |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| D1  | a route carrying an authentication guard has an operation that declares `security(...)`                                                                  | undocumented authentication: a reviewer reading the document cannot tell the route is guarded                                               |
| D2  | an operation declaring `security(...)` has a route carrying an authentication guard                                                                      | the dangerous direction: the document claims a requirement the code does not enforce                                                        |
| D3  | the guard argument matching that authentication class is propagated in the body (C1 applied to this class)                                               | security declared, guard present, rejection dropped — the `get_rows` shape end to end                                                       |
| D4  | `securitySchemes` defines every scheme any operation references                                                                                          | a dangling scheme name is a document that no generator can render                                                                           |
| D5  | a route carrying the explicit re-authentication guard declares it in the document (extension today; a second scheme if that question goes the other way) | the dangerous direction: the operation silently needs a password the document never mentions, so a client cannot perform it                 |
| D6  | the re-authentication guard appears **with** an authentication guard, never alone                                                                        | a password check without a token is either a second authentication factor or an unauthenticated endpoint, and only one of those is intended |
| D7  | the guard-provided credential is compared in constant time                                                                                               | `update_password_handler` compares with `!=` (`edit_config.rs:157`); the guard that replaces it must not carry the same habit forward       |

### M — the mode guard (`GuardReadOnlyMode`)

Not authentication, and it must never be documented as such. `read_only_mode` is
a **server-side setting** (`APP_CONFIG`, settable at startup through
`PICASU_READ_ONLY_MODE` — `backend/src/model/config.rs:403-408`, and through
`PUT /put/config` while the mode is off). The guard consults it and rejects with
**405 Method Not Allowed** and `ErrorKind::ReadOnlyMode` — a status that is not
an authentication failure, which is the clearest sign it belongs to another
class. Handlers consume it as `GuardResult<GuardReadOnlyMode>` and immediately
`let _ = read_only_mode?;`: the value is a unit struct, so the guard's only
effect is the rejection.

So the guard is the route's own declaration that **this route mutates state**,
and the setting decides whether that is currently permitted. Two consequences for
the document: the requirement is a documented `405`, not a credential; and a
client cannot satisfy it with any header or token, so putting it in `security`
would tell generated clients to send something the server ignores and believe
they are permitted when they are not.

| #   | assertion                                                            | calibration                                                                                                                                                                                                                                                   |
| --- | -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| M1  | every route with a mutating method (`PUT`, `POST`) carries the guard | **13/13 PUT and 4/4 POST routes carry it, zero exceptions.** `DELETE` has no routes yet, and the login/token-renewal routes live in `auth.rs` and mount separately, so they are outside these groups. The rule needs no heuristic and no exception list today |
| M2  | a route carrying the guard documents a `405` response                | **16 findings today.** Exactly one operation documents it — `POST /post/rebuild`, which established the convention as `(status = 405, description = "Read-only mode")` — while the other 16 mutating routes can answer 405 and list only 200/400/401          |
| M3  | the guard never appears in `security(...)`                           | no findings today, because no operation declares `security` at all                                                                                                                                                                                            |
| M4  | the guard's argument is propagated with `?`                          | C1 restated for this class, so the rule reads where the class is described                                                                                                                                                                                    |

**M1's exception list is empty today and will not stay empty.**
`.plan/bug-readonly-lockout.md` exists because `PUT /put/config` — which
carries the guard — is also the only route that can set the mode to `false`, so
once the mode is on, the API cannot lift it. The fix introduces a
re-authentication route that must stay reachable precisely because the mode is
on; that route is the first named exception to M1, and the exception lives in
that plan rather than here. `PUT /put/config/password` is the open question: it
carries the guard today, and the same argument applies to it.

### E — deferred, and why

- **A crate-wide function-signature index** to enforce "a handler holding
  `GuardReadOnlyMode` must not call a function taking `&mut Config`". This is the
  one rule that genuinely needs cross-function analysis (an index of
  `fn → parameter types` by walking the crate's modules; no call graph needed).
  It is deferred because it needs a short, explicit list of mutating entry
  points, and without one it will produce false positives on every save helper.
- **A document linter** (Spectral or Redocly) for the document-shaped rules the
  deleted checker used to carry: tag vocabulary, `operationId` uniqueness and
  stability, `$ref` resolution, orphan schemas, descriptions. Until one is
  adopted, A3 and A6 are the only tag/operationId coverage this repository has.

### Where this runs

As backend tests, not build warnings. A build-time warning was what the deleted
crate emitted and nothing read them; a failing test blocks. `syn` is needed only
to read attributes and to walk function bodies. The scan support for the two
rules in section C is already ~400 lines, and A, B and D add to the same walker
rather than to a new one, so the estimate for the whole list is 1 200–1 500
lines. That does not justify a crate of its own, and it should not go near the
production binary. `backend/src/tests/` gains one module and `syn` arrives as a
dev-dependency.

### Sequencing and acceptance

1. **C1** first: it is the only rule with a proven incident behind it, and it
   ships with a mutation test that fails when the rule is removed.
2. **A1–A6** as one module: mechanical, calibrated against the tree, and they
   fail the branch if any of the 63 annotations regresses.
3. **B1–B3**, then **D1–D4** once the decision on `security(...)` versus the
   per-class extension is taken and the schemes are registered in `ApiDoc`.
   **M1, M3, M4** land with them — all three are green today — and **M2 lands
   with the 16 missing `405` responses added in the same change**, because a gate
   that starts with sixteen findings is a gate people learn to ignore.
4. **C2** last; it is a message-quality improvement over a gate that already
   catches the condition.

Acceptance: every rule has a test that fails when the rule is deleted (mutation
style, no exceptions); the suite is green against the current tree with zero
findings except where a finding is the expected demonstration; `just test` and
`just check` are green; `docs/openapi-generator.md` states which rules are
enforced here and which are review-time, so the boundary is written down rather
than remembered.

## Progress

### 2026-10-01 — C1 and C1b landed (uncommitted, for review)

Increment 1 of the sequencing: the two handler-body rules. Drafted as backend
tests in `backend/src/tests/openapi_annotations.rs` (rules and fixtures) and
`backend/src/tests/openapi_annotation_scan.rs` (the `syn` scan), with `syn`
3.0.5 and `proc-macro2/span-locations` added as dev-dependencies. The drafts
never landed, so those two paths record where the work was written rather than
files to go and read.
The scan reads `src/router` through `walkdir`; findings render as
`path:line: handler: what is wrong`.

Zero findings across the tree, and the calibration constants are pinned so the
scan cannot go quiet by walking less:

| fact                      | pinned value | where it is checked                                 |
| ------------------------- | ------------ | --------------------------------------------------- |
| annotations               | 63           | `guard_propagation_is_clean_across_the_router_tree` |
| `GuardResult<…>` bindings | 52           | same test                                           |
| plain `Guard…` bindings   | 9            | same test                                           |

**Correction to C1b's calibration:** the tree has **nine** plain-guard bindings,
not five — `album_index.rs` ×3, `edit_config.rs` ×2, and one each in
`rebuild.rs`, `import_config.rs`, `get_fs_completion.rs`, `get_album_index.rs`.
All nine are `_auth: GuardAuth` and none of them is touched in the body. The
conclusion of C1b is unaffected; only the count was wrong, and the test pins 9.

**Open reading of C1, taken literally and worth a review decision:** the
propagating positions are the operand of `?`, the scrutinee of `match` /
`let` / `if let`, an argument of another call, and a returned value (including
the trailing expression). A two-step `let carried = auth; … carried?;` is
therefore a finding even though a human would accept it; neither it nor any other
shape occurs in the tree today. A `move` into a closure is a different case and is
**accepted** when the closure propagates (`spawn_blocking(move || { … auth?; … })`),
because the walker recurses into closure bodies. Both are pinned as tests:
`rebound_guard_result_is_reported` and `guard_moved_into_a_closure_is_accepted`.

Guard classification is name-based: `GuardResult<…>` carries the obligation, any
other type whose last segment starts with `Guard` carries none, and anything else
is out of scope. Note that `renew_hash_token` binds `TimestampGuardModified`,
which is a plain Rocket guard that does **not** match the `Guard` prefix. That
is harmless for C1/C1b (it is not a `GuardResult`), but the D-series rules will
have to identify guard classes by something other than a name prefix.
