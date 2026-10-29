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
| `effects`    | manage/effect classification (**pending**: all `[]`)                                                                       |
| `invariants` | `authz-model.md` IDs, only where source-verified                                                                           |

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
   the TOML configs, pinned to the matrix by a consistency test. The two
   campaign tiers remain advisory (`just fuzz-check`, `just fuzz-run`),
   compared against `utils/schemathesis/baseline.yaml`.
3. **Generated negatives.** Token×guard cross-product with expected
   401/403/404 per cell. Partly realized: the probe plan assigns one
   cross-scope / wrong-typ / renewal-mismatch / demoted probe per matching
   operation (28 operations today), but each operation still carries a
   single probe rather than a full cross-product. _(Partial.)_
4. **`authz-model.md`.** Stays the prose authority for AUTH-1..16
   until the annotation-based model can derive its docs back; at
   that point the model replaces the hand-maintained file. Shape to
   be settled when the matrix lands. _(Placeholder.)_

## Runners and gates

| Command                             | Scope                                    | Gate                                  |
| ----------------------------------- | ---------------------------------------- | ------------------------------------- |
| `just openapi-check`                | spec parity, shape, lint, routes         | gating (CI + precommit on `backend/`) |
| `just dylint-check`                 | AUTH-2, AUTH-3, decode agreement         | gating (CI)                           |
| `just fuzz-check` / `just fuzz-run` | 5-check oracle sweep / stateful campaign | advisory; baseline-compared           |
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
