---
status: in-progress
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

Per annotation, no cross-file knowledge. The "no false positives found against the
current tree (63 annotations)" calibration below was written before A4 and A5
were run, and it was wrong: those two fired 50 and 8 times. What survives it is
recorded per rule in the progress notes.

| #   | assertion                                            | why nothing else enforces it                                                                                      |
| --- | ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| A1  | no `path = "..."` and no bare verb                   | the convention exists only in review; nothing rejects a re-introduced duplicate                                   |
| A2  | `responses(...)` present and non-empty               | utoipa invents no response, so the omission is silent                                                             |
| A3  | exactly one `tag`, from the closed vocabulary of ten | the vocabulary is documented in `docs/openapi-generator.md`; its checker was deleted                              |
| A4  | the handler carries a doc comment                    | `summary`/`description` are derived from it, so the omission is invisible except in the reference                 |
| A5  | the summary is one line                              | a multi-line summary splits the generated reference's headings — measured, not stylistic                          |
| A6  | no hand-set `operation_id`                           | utoipa derives it from the function name; a hand-set one is the only name nothing compares                        |
| A7  | no hand-set `summary` or `description`               | utoipa derives both from the doc comment; a hand-set one is the same prose twice, and nothing compares the copies |

**A3 has no exemptions, and the vocabulary absorbed the awkward case.** The two
test-only probes (`probe_record`, `probe_dupe_group`) are stripped from the
published document by `openapi_public`, so a reader of the reference cannot reach
them and a tag naming a public subject would misfile them. The vocabulary's tenth
entry, `internal`, says "outside the published API" instead: a rule with an
exemption is a rule with a way round it, and a tag in a list is a fact a reader of
the source can see where a reader looks. Every other rule applies to the probes as
before, because the contract tests read the _full_ spec rather than the public one.

**The tool holds no backend facts, and A3 is where that was nearly lost.** An
earlier version exempted the excluded routes by carrying
`CONTRACT_EXCLUSION_PREFIXES` as a copy in `openapi-sanity` (see the superseded
progress entry below). The copy was a second place to forget, and the rule around
it needed a route path, which is the shape of a fact belonging to the backend. The
durable rule for the tool is now written at the top of `utils/openapi-sanity`'s
module docs and in its README: **it holds annotation conventions, and if a rule
seems to need a route, a prefix, a config value or a feature name, that is a sign
it belongs in the backend or in a just recipe.** `TAGS` is the one set the tool
enforces that is written down elsewhere, and it is a convention — a table in
`docs/openapi-generator.md` — not a fact read out of the backend.

**A7 exists because A5's premise is false without it.** utoipa lets an
annotation set `summary` and `description` outright, and one annotation did
(`put/assign_album.rs`, `summary` and `description` carrying the same prose as
the doc comment beside them). While that is possible, "the first paragraph _is_
the operation's summary" does not hold, and A5 was reporting a defect the
generated document did not have. A7 closes it with A6's argument — a hand-set
name is the one nothing compares, and here the name is prose.

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

As a standalone tool, `utils/openapi-sanity`, not as build warnings and not as
backend tests. Both of those were tried: the deleted crate emitted a build
warning and nothing read it, and the rules as backend tests only ran in CI,
because the pre-commit hook runs `just backend-check` and `just openapi-check`
for a `backend/` change but not the test suite. The tool is the first phase of
`just openapi-check`, so it runs on the same `backend/` change that can introduce
the defect. It reports one `file:line: message` per finding, a summary line
naming the count, and exits non-zero on findings — the reporting contract the
other two `openapi-check` phases already use.

`just openapi-sanity` runs it over `backend/src/router`; `--source-root <dir>`
points it at any other tree. The phase also passes `--expect-at-least 60`, a floor
on the annotated handlers the scan must see before a clean run means anything: a
walk that stopped descending reports the same emptiness as a clean tree, so a
short scan fails instead of reporting clean. 60 is headroom below the tree's 63,
so a new handler does not break the gate. Rules live in the crate's `src/lib.rs`
so the CLI and the tests in `tests/openapi_annotations.rs` check the same code, and
`syn` with `proc-macro2/span-locations` is a normal dependency of the tool rather
than a dev-dependency of the backend. It stays out of the production binary either
way. The scan support for the two rules in section C is already ~400 lines, and A,
B and D add to the same walker rather than to a new one, so the whole list lands
in one tool.

**The name is reused deliberately.** The crate is named after the analyzer deleted
in `e1ec74da` because it does the same job — a source-level gate on these
annotations — but it is a new tool with a new rule set. None of that crate's rules
come back: no tag policy, no `AUTH_POLICY` table, no route-set rules, no
source/spec comparison. Its rule set is this plan, and only section C is
implemented.

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

### 2026-10-02 — the backend facts are out of the tool (uncommitted, for review)

**Supersedes the A3 exemption in the next entry down.** The exemption was the
right shape of _fix_ and the wrong _place_ for it: `EXCLUDED_ROUTE_PREFIXES` was a
copy of the backend's
`CONTRACT_EXCLUSION_PREFIXES`, and the rule around it had to read a route path to
apply. That is a backend fact living in a crate whose job is annotation
conventions, and a copy of a path list is a second place to forget — the gate
built on it would report a stale list rather than the truth.

So A3 is absolute again: every annotated operation declares exactly one tag from
the vocabulary, and the tenth entry `internal` is how an operation outside the
published API says so. `EXCLUDED_ROUTE_PREFIXES`, `AnnotatedHandler::route_path`,
`AnnotatedHandler::is_excluded`, `ROUTE_VERBS`, the `a3_excluded_prefixes.rs`
fixture and its two tests are gone; the two probes carry `tag = "internal"`.

The principle is now written where the next increment will read it: the module
docs at the top of `utils/openapi-sanity/src/lib.rs`, the tool's README, and the
comment on the `openapi-sanity` recipe in the justfile each say that the tool
holds conventions and no backend facts, and that a rule needing a route, a prefix,
a config value or a feature name belongs in the backend or in a recipe. That
sentence is the durable answer to an agent helpfully adding a backend detail to
the checker, which is the only way this mistake gets made.

### 2026-10-02 — A1–A7, the probe exemption, and `trace` (uncommitted, for review)

> **Superseded:** the A3 exemption below was replaced by the `internal` tag, and
> the code it describes no longer exists. A7 and `trace` in this entry stand.
> The entry is kept because the reasoning it records — a rule with an exemption is
> a rule with a way round it — is the reasoning the replacement rests on.

Follow-up to the A1–A6 increment below, from its review. Three changes, none of
which weakens a rule:

- **A3 exempted the excluded surface.** `EXCLUDED_ROUTE_PREFIXES` in the tool was a
  copy of the backend's `CONTRACT_EXCLUSION_PREFIXES`, and A3 skipped a handler
  whose route attribute was under one of them. The two test probes stopped being
  findings, so the tree was silent. The exemption was in the tool and not on the
  command line on purpose: a `--exclude-prefix` flag would spell the prefixes a
  third time, in the justfile, which is what the backend's constant exists to
  avoid. `a3_excluded_prefixes.rs` carried two exempt handlers _and_ a
  non-exempt one without a tag, and `a_tag_is_not_required_on_a_route_the_published_document_drops`
  asserted all of it in one render — the control is what stopped the exemption from
  being a way to switch the rule off. A handler with no route path was not exempt.
  **All of this was reverted**; see the entry above.
- **A7 — no hand-set `summary` or `description`.** Added because the review
  confirmed the A5 gap: `put/assign_album.rs` carried both, saying what its doc
  comment said, and while an annotation may set `summary`, "the first paragraph
  _is_ the summary" is false. `grep` over every `#[utoipa::path]` body in
  `backend/src/router` found **exactly one** override — `assign_album` at lines
  77 and 78 — so it was removed and its doc comment extended with the one clause
  the override carried that the comment did not ("or the destination album is a
  manual album"). Nothing else in the tree overrides either key, so nothing was
  deleted on a guess. `assign_album`'s `summary` in the document is now derived.
- **`trace` added to A1's verb list.** It is a Rocket verb and utoipa's
  `HttpMethod` accepts it as a bare token; the plan's list of eight omitted it by
  oversight. No annotation uses it, so the tree is unaffected and a restatement
  can no longer slip through the gap.

### 2026-10-02 — A1–A6 landed (uncommitted, for review)

Increment 2 of the sequencing: the six annotation-shape rules, as one module of
`utils/openapi-sanity` beside C1/C1b. Each is a small function over an
`AnnotatedHandler`; `scan_source_root` runs all of them, so the CLI and the
suite check the same code. One fixture per rule plus a conforming counterpart per
rule, all `include_str!`.

**Section A's own claim of "no false positives found against the current tree" is
wrong, and finding that is most of what this increment cost.** Three of the six
rules fired on the 63 annotations; the source was fixed in every case, no rule was
weakened:

| Rule | Findings on the tree | Fix                                                              |
| ---- | -------------------- | ---------------------------------------------------------------- |
| A1   | 0                    | —                                                                |
| A2   | 0                    | —                                                                |
| A3   | 2                    | resolved in review: the tenth tag, `internal`                    |
| A4   | 50                   | a doc comment on every annotated handler that had none           |
| A5   | 8                    | the first paragraph of eight doc comments reflowed onto one line |
| A6   | 0                    | —                                                                |
| A7   | 1                    | the `summary` / `description` override in `assign_album` removed |

A4 is the largest change in the branch and the most consequential: **49 of the
61 operations in `backend/openapi.json` had no `summary` before it.** The
annotation declares no `summary = "…"`, so utoipa derives the field from the
doc comment, and 50 of the 63 annotated handlers had none. 49 of the 50 handlers
that were missing one reach the document; the two test probes do not, which is
why the number is 49 and not 50. The document still looked complete, because an
operation with a `paths` entry, a `responses` map and a `tags` array is
indistinguishable from a documented one until a reader looks for prose.

A5's eight findings were the six multi-line summaries the document already had,
plus two in handlers whose operations the public-spec filter drops. The fix was
to move the wrapped sentences into a second paragraph rather than to lengthen one
line, so the `description` keeps everything the `summary` used to say. This is
also the rule with the one calibration worth recording:

- **`put/assign_album.rs` sets `summary = "…"` and `description = "…"` in the
  annotation**, which overrides the derivation A5 is about — so its multi-line
  doc comment produced a finding against a generated summary that was one line and
  correct. The doc comment was reflowed like the other seven, and the review that
  followed closed the gap with A7 rather than with an exception to A5: the
  overrides are gone and the doc comment is the single source.

**The A3 question is now answered** — by the tenth tag, as recorded in the review
entries above. `probe_record` and `probe_dupe_group` carry `tag = "internal"`, and
the rule has no exemptions.

Two smaller notes for the same review:

- **The vocabulary is a copy.** `TAGS` in the tool duplicates the table in
  `docs/openapi-generator.md`; the document cannot be parsed for it, because the
  document is generated from the annotations. The two are changed together, and
  both places now say so. It is a _convention_ written down in prose, which is a
  different thing from a backend constant, and the review that followed removed
  the one set that was the latter.
- **A1/A3 leave a residue.** `method(GET)`, `tags([…])` and `context_path` are
  spellings utoipa accepts for the same facts, none of which the rules reject and
  none of which the tree uses. `trace` was in this list until the review moved it
  into A1's verb list. The remainder is named in the tool's module docs, its
  README and `docs/openapi-generator.md` so the boundary is written down rather
  than remembered.

`backend/openapi.json` is regenerated in this change, and
`docs/openapi-generator.md` now states the enforced rules, the three-phase gate
and the corrected authoring steps.

### 2026-10-01 — the rules moved to `utils/openapi-sanity` (uncommitted, for review)

The two rules kept their behaviour and their calibration and moved house. The
backend placement was wrong for one reason: the pre-commit hook runs
`just backend-check` and `just openapi-check` for a `backend/` change and not the
test suite, so the rules only ran in CI. They are now `utils/openapi-sanity` — a
tool, first phase of `just openapi-check` — and `backend/Cargo.toml` no longer
carries `syn`.

Two holes in that arrangement were closed in the same change, because the phase
now runs where the rules used to be blind. The phase passes a handler floor
(`--expect-at-least 60`), so a scan that stopped descending fails instead of
reporting a clean tree — the CLI alone cannot tell that difference, only a floor
supplied from outside can. And the hook's `utils/` branch runs `just utils-test`,
so a change to the rules or the walk is exercised by its own suite before it
reaches CI rather than after.

The crate name is the deleted analyzer's name on purpose, which makes the old
"as backend tests" note above a historical record rather than the current state.

### 2026-10-01 — C1 and C1b landed (uncommitted, for review)

Increment 1 of the sequencing: the two handler-body rules. Implemented as
backend tests in `backend/src/tests/openapi_annotations.rs` (rules and
fixtures) and `backend/src/tests/openapi_annotation_scan.rs` (the `syn` scan),
with `syn` 3.0.5 and `proc-macro2/span-locations` added as dev-dependencies.
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
