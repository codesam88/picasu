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

Seven guard classes exist, and they are two different things:

- **Authentication** (they reject a caller who cannot prove who they are, and
  belong in the document): `GuardAuth`, `GuardTimestamp`, `GuardHash`,
  `GuardHashOriginal`, `GuardShare`, `GuardUpload`.
- **Mode restriction** (they constrain what this build may do, and are not
  authentication): `GuardReadOnlyMode`.

The design, in two parts:

- **Standard, consumer-visible.** Register one security scheme in the document
  and declare `security(("bearer_auth" = []))` on every operation whose route
  carries an authentication guard. `securitySchemes` does not exist in the
  document yet and must be added to `ApiDoc`'s `components(...)`.
- **Repo-specific class, only if the review wants it in the document.** An
  extension in the shape already used for features —
  `extensions(("x-picasu-auth" = json!("GuardTimestamp")))` — would say _which
  kind_, which `security` cannot express. **Open question for the user:** start
  with `security(...)` alone (standard, visible to any consumer, checker ties it
  to the route and the handler) or also carry the per-class extension. The
  recommendation is to start with `security(...)`, because it is the signal a
  consumer and a generator can both act on, and to add the extension when a
  reviewer asks which token a route expects.

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

| #   | assertion                                                                                                                                                                                        | calibration                                                                                                                                                                                                                                                                                                                    |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| C1  | **a guard-provided argument must have its rejection propagated**: the binding appears as `ident?`, is matched, or is forwarded. `let _ = ident;` and a binding absent from the body are findings | the tree uses `let _ = auth?;` / `let _ = read_only_mode?;` in ~100 places, so the conforming idiom is `let _ = ident?;` and the rule fires on the `84f29aa5` shape. Zero findings today; it earns its place as a regression guard and must be proved by a mutation test (the deleted suite's `dropping_a_guard_result_fails`) |
| C2  | every `pub fn generate_*_routes()` is mounted in `router/builder.rs`                                                                                                                             | not coverage — `--check-openapi` catches an unmounted group — but the gate's message ("documented operation, no route mounts") sends the reader to the document instead of to `builder.rs`                                                                                                                                     |

### D — the guard and `security` rules

| #   | assertion                                                                                                  | direction that matters                                                                        |
| --- | ---------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| D1  | a route carrying an authentication guard has an operation that declares `security(...)`                    | undocumented authentication: a reviewer reading the document cannot tell the route is guarded |
| D2  | an operation declaring `security(...)` has a route carrying an authentication guard                        | the dangerous direction: the document claims a requirement the code does not enforce          |
| D3  | the guard argument matching that authentication class is propagated in the body (C1 applied to this class) | security declared, guard present, rejection dropped — the `get_rows` shape end to end         |
| D4  | `securitySchemes` defines every scheme any operation references                                            | a dangling scheme name is a document that no generator can render                             |

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
to read attributes and to walk function bodies — roughly 250 lines for the whole
list — so it does not justify a crate of its own, and it should not go near the
production binary. `backend/src/tests/` gains one module and `syn` arrives as a
dev-dependency.

### Sequencing and acceptance

1. **C1** first: it is the only rule with a proven incident behind it, and it
   ships with a mutation test that fails when the rule is removed.
2. **A1–A6** as one module: mechanical, calibrated against the tree, and they
   fail the branch if any of the 63 annotations regresses.
3. **B1–B3**, then **D1–D4** once the decision on `security(...)` versus the
   per-class extension is taken and the schemes are registered in `ApiDoc`.
4. **C2** last; it is a message-quality improvement over a gate that already
   catches the condition.

Acceptance: every rule has a test that fails when the rule is deleted (mutation
style, no exceptions); the suite is green against the current tree with zero
findings except where a finding is the expected demonstration; `just test` and
`just check` are green; `docs/openapi-generator.md` states which rules are
enforced here and which are review-time, so the boundary is written down rather
than remembered.
