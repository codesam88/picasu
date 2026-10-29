# API Assurance

> **Purpose and audience.** This document is the spec of the API
> assurance strategy and the tooling that implements it, for
> developers and security reviewers. Assurance runs in two
> directions: checks mechanically keep the API specification close to
> the implementation, and generators drive tests and verification
> tools from the specification. This page is the permanent record of
> the _currently implemented_ strategy — anything planned but not yet
> built is marked as such. The implementation roadmap lives in
> `.plan/api-assurance.md` and is not repeated here except as status
> labels.

## Strategy

1. **Spec ← implementation.** Every behavioral claim the published
   contract makes about authentication is checked against the handler
   that answers it. A mismatch is a finding in CI, not a review
   observation.
2. **Tests ← spec.** The committed contract feeds configuration and
   fixtures into black-box testing (Schemathesis) and generated
   negative tests, so the handwritten suites pin flows while derived
   suites cover the cross-product.

## Contract foundations (implemented)

The OpenAPI document is generated from `#[utoipa::path]` annotations
(`#[utoipauto]` collector) and held to the implementation by
`just openapi-check`: source-shape rules (`openapi-sanity`), a
regeneration diff, Spectral lint, and route-set parity
(`picasu --check-openapi`, 74/74 routes matched). Breaking changes
fail via `openapi-breakcheck`. See `docs/openapi-generator.md` for
the pipeline.

## Auth metadata model

Per-operation authorization semantics that OpenAPI `security`
cannot express live in an `x-picasu-auth` extension next to the
native `security(...)` clause:

| Field        | Meaning                                                                                                                    |
| ------------ | -------------------------------------------------------------------------------------------------------------------------- |
| `guards`     | handler signature guard types, verbatim                                                                                    |
| `tokenTyp`   | credential types accepted (`session`, `snapshot`, `asset`, `share`)                                                        |
| `admin`      | `admin` (admin-only), `any`, `none`                                                                                        |
| `shareScope` | `album` (target confined to the share's album), `n-a` (no album binding) — always token-derived, never a request parameter |
| `public`     | no credential required (pairs with `security(())`)                                                                         |
| `effects`    | mutation class, exactly one: `read`, `write` (content), `manage` (administrative); empty for public operations             |
| `invariants` | `authz-model.md` IDs the operation is held to, checked for existence and against the guards                                |

All 51 annotated handlers carry the key; the 23 SPA page shells are
unannotated by design. `effects` has no controlled vocabulary yet;
`invariants` backfill waits on the manage/effect classification.

## Correspondence rules

| Rule | Claim checked                               | Enforced by                                   | Status                                    |
| ---- | ------------------------------------------- | --------------------------------------------- | ----------------------------------------- |
| S1   | metadata present ⇔ `security(...)` present  | `openapi-sanity`                              | gating                                    |
| S2   | `security` schemes ⇔ guard types            | `openapi-sanity`                              | gating                                    |
| S3   | `tokenTyp` ⇔ decoded claims type            | dylint (`token_typ_agreement`, gating)        | implemented; gating in CI                 |
| S4   | asset touched is bound to the share's album | behavioral cross-scope probes, matrix-derived | implemented; advisory via `just fuzz-run` |
| S5   | `readonly` ⇔ guard + 405 declared           | `openapi-sanity`                              | gating                                    |
| S6   | `reauth` ⇔ guard + documented header        | `openapi-sanity`                              | gating                                    |
| S7   | `public` ⇔ `security(())` + no guard        | `openapi-sanity`                              | gating                                    |

Split rationale: annotation/signature comparisons belong to the syn
checker, type/call-graph questions to dylint, binding semantics to
behavioral tests. Each rule lives where its inputs live.

## Consumers (planned unless noted)

1. **Auth matrix (implemented).** `scripts/auth-matrix.mjs` emits the
   human table (`docs/auth-matrix.md`, the review baseline) and the
   machine file (`backend/auth-matrix.json`, per-operation records
   for downstream consumers) from the committed spec; `auth-matrix-check`
   diffs both in CI as an `openapi-check` phase. The reference still
   generates from `openapi.json` directly; the matrix stays a separate
   review artifact unless a future step merges them.
2. **Schemathesis derivation (implemented).** The campaign hooks derive
   their auth knowledge from `backend/auth-matrix.json` through
   `utils/schemathesis/auth_matrix.py`: the share-header injection set,
   the probe each operation carries, wrong-typ token selection, read-only
   exemptions, and the seeded scope pairs. The derivation is pure and
   unit-tested; a matrix change that alters a probe fails the tests
   instead of drifting. Destructive-operation disables stay hand-tuned in
   the TOML configs, pinned to the matrix by a consistency test. Tier 1
   (`just fuzz-check`) gates CI against `utils/schemathesis/baseline.yaml`;
   the stateful campaign (`just fuzz-run`) stays advisory because its phases
   are time-budgeted and need a served instance.
3. **Generated negatives (implemented, role layer).** `backend/src/tests/matrix_negatives.rs`
   reads the published document and asserts, per operation and principal,
   that the model's exclusions hold: anonymous is refused on every
   non-public operation, a non-admin session is refused on admin-only
   operations, a session never satisfies a bearer-only operation, and a
   viewer is admitted where the model says `any` with a session presenter.
   Refusals are judged as any 4xx, because `GuardResult` defers the refusal
   to the handler and a body guard can answer first; the run reports how
   many refusals happened at the credential layer (401). Operations whose
   guards or invariants can refuse beyond the role — reauthentication and
   body-token renewal — carry no admission assertion. Public operations and
   share credentials are out of scope: their verdicts depend on handler
   logic and seeded albums, covered by the scenario suite. What is still
   missing is the full token×guard cross-product: each operation carries one
   principal check, not every cell. _(Partial.)_
4. **`effects` and `invariants` (implemented).** Each operation declares one
   mutation class — `read`, `write` or `manage` — and the invariants it is
   held to. Two source rules keep the declaration honest: `manage` requires
   the admin role (AUTH-11) and `write` requires the read-only gate
   (AUTH-12); the matrix test checks that every class is in the vocabulary,
   that exactly one is declared, and that every referenced invariant exists
   in `authz-model.md`. AUTH-12 is complete: `manage` requires the read-only
   gate too, and the one operation that must stay available while read-only
   is on — lifting the kill-switch — is classed `control` rather than
   exempted by path.
5. **`authz-model.md`.** Stays the prose authority for AUTH-1..16
   until the annotation-based model can derive its docs back; at
   that point the model replaces the hand-maintained file. Shape to
   be settled when the matrix lands. _(Placeholder.)_

## Runners and gates

| Command                             | Scope                                    | Gate                                  |
| ----------------------------------- | ---------------------------------------- | ------------------------------------- |
| `just openapi-check`                | spec parity, shape, lint, routes         | gating (CI + precommit on `backend/`) |
| `just dylint-check`                 | AUTH-2, AUTH-3, decode agreement         | gating (CI)                           |
| `just fuzz-check` / `just fuzz-run` | 5-check oracle sweep / stateful campaign | sweep gating in CI; campaign advisory |
| `just env-var-check`                | config consistency                       | gating                                |

Quarterly re-pin cadence applies to Schemathesis, dylint nightlies,
and the other pinned tools. Research digests live in `docs/briefs/`.

## Open gaps

- `effects` controlled vocabulary and manage/effect classification
  (AUTH-11/12).
- `invariants` backfill beyond the source-verified set.
- Query-param share fallback is invisible to the published schemes.
- Single `bearer_token` scheme covers three token types; `tokenTyp`
  is authoritative until schemes split.
