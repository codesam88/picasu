---
status: open
type: chore
priority: medium
area: testing
---

# Contract checker coverage and strength

## Problem

The contract is machine-readable, but nothing says which of its _elements_ a
checker actually covers, or how strong that checker is. Two symptoms:

- Coverage is uneven and invisible. `credentials` is checked against HIR types
  (dylint) while `guards` is compared to nothing (every static rule reads the
  handler signature, never the array), `admin` exists only as a behavioural
  expectation, `shareScope` is advisory, and `effects`/`invariants` have no
  checker at all.
- Counting rules or tests measures volume, not coverage. A rule can be
  vacuous when its expectation is derived from the same artifact it checks,
  and an advisory check does not gate. Neither is visible today.

This applies beyond `x-picasu-auth`: large parts of `backend/openapi.json`
(route set, statuses, parameters, bodies, schemes, schemas) are covered by
different tools with different strengths, and no artifact states which.

## Goal

One registry that maps each contract element to the checker(s) that cover it,
with an explicit strength and independence judgement, enforced so it cannot
rot:

- a **registry** (`utils/api-assurance/contract-checks.json`): element →
  A-side rules, B-side checks, strength, independence, gate, waiver/reason;
- an **existence test**: every referenced rule id, check id and test name must
  exist in the tree (the `tags.json` discipline), so a deleted checker fails
  instead of silently leaving a claimed coverage;
- a **generated profile** (`docs/assurance-profile.md`) rendered from the
  registry and diff-gated like `auth-matrix.md`, so changes are reviewable;
- an opt-in **floor**: no element below a chosen strength without a recorded
  waiver, and no element whose only checker is advisory without one. The
  measured number (elements at or above the floor) is the coverage-strength
  metric.

### Strength model

Two dimensions; coverage alone misleads.

- **Layer**: static (source inspection) or behavioral (runtime assertion).
- **Exactness**: `none` < `shape` (presence/format) < `static-exact`
  (agreement with an independent source) < `behavioral-gating` <
  `behavioral-advisory`.
- **Independence**: whether the expectation comes from different data than
  the artifact under test. A rule whose expectation is derived from the same
  annotation is vacuous for that annotation and is recorded as `derived`, not
  counted at full strength.

## Preliminary findings (verified against the tree, 2026-10-29)

| Contract element                                  | A-side (static)                      | B-side (behavioral)         | Strength             | Note                                                          |
| ------------------------------------------------- | ------------------------------------ | --------------------------- | -------------------- | ------------------------------------------------------------- |
| mounted route ⇔ documented operation              | `--check-openapi`                    | —                           | static-exact, gate   |                                                               |
| committed spec ⇔ generator                        | `openapi-json-match`                 | —                           | static-exact, gate   |                                                               |
| document validity                                 | Spectral (`.spectral.yaml`)          | —                           | static, gate         | warnings do not fail                                          |
| breaking-change compatibility                     | oasdiff (`openapi-breakcheck`)       | —                           | static-exact, gate   |                                                               |
| annotation shape (path/verb/tag/doc)              | A1–A7, A9                            | —                           | shape, gate          |                                                               |
| parameters ⇔ route bindings                       | B1, B2, B5                           | —                           | static-exact, gate   |                                                               |
| request body schema ⇔ parsed type                 | B3, B4                               | —                           | static-exact, gate   |                                                               |
| declared statuses ⇔ handler universe              | P1–P4                                | fuzz conformance (advisory) | static-exact, gate   | errors checked partially                                      |
| tag vocabulary                                    | A3 + `tags.json` mirrors             | —                           | static-exact, gate   |                                                               |
| `security` schemes ⇔ guard types                  | S2                                   | A1/A3/A5 probes             | static-exact, gate   |                                                               |
| `security` present ⇔ extension present            | S1 (both ways)                       | —                           | shape, gate          |                                                               |
| `public` ⇔ `security: [{}]` + no guard            | S7                                   | —                           | static-exact, gate   |                                                               |
| `guards` array ⇔ handler signature                | **none**                             | indirect (role rendering)   | **shape at best**    | no rule compares the array to the signature                   |
| `admin` ⇔ `GuardAdmin`/`GuardUser`                | **none**                             | `matrix_negatives`          | behavioral, gate     | only B-side                                                   |
| `credentials` ⇔ decoded claims type               | S3 dylint (HIR)                      | A3 probe                    | static-exact, gate   | strongest field                                               |
| `shareScope` ⇔ enforced album binding             | none                                 | S4 probes                   | behavioral, advisory | does not block merges                                         |
| `effects`                                         | none                                 | none                        | **none**             | all `[]`                                                      |
| `invariants`                                      | none                                 | AUTH-10 in one probe        | **shape**            | not validated against `authz-model.md`; 11 of 16 unreferenced |
| response schemas ⇔ serialized types               | none                                 | fuzz schema conformance     | behavioral, advisory | no source-level agreement check                               |
| `securitySchemes` definitions ⇔ credential shapes | none (registered by `SecurityAddon`) | A1/A3/A5                    | shape                | one `bearer_token` scheme covers three token types            |
| share query-param fallback vs declared headers    | none                                 | —                           | **none**             | documented gap                                                |
| servers / links / examples                        | absent by design                     | —                           | n/a                  | Spectral rules off with reasons                               |

## Steps

1. **Registry**: add `utils/api-assurance/contract-checks.json` with the rows
   above; each entry names concrete rule ids (e.g. `openapi-sanity:S2`),
   check ids (`matrix_negatives:viewer-on-admin`), strength, independence,
   gate and optional waiver with a reason.
2. **Existence test**: `utils/api-assurance/test_contract_checks.mjs` verifies
   every referenced rule appears in `openapi-sanity`'s rule table, every check
   id in the named test/hook, and every waiver carries a reason; wire into
   `just utils-test`.
3. **Generated profile**: `scripts/assurance-profile.mjs` renders
   `docs/assurance-profile.md` from the registry, diff-gated as an
   `openapi-check` phase (the `auth-matrix-check` pattern).
4. **Close the two weakest rows** before setting a floor: `guards` ⇔ signature
   (S8) and `invariants` ⊆ declared AUTH ids.
5. **Floor**: after 1–4, set the minimum strength per element class and fail
   the check on a regression; record the metric (elements at or above the
   floor) in the generated profile.

## Open decisions

- Floor level: `static-exact` for every field, or allow behavioural-gating
  where static agreement is not decidable (e.g. `shareScope`)?
- Whether the profile belongs in `docs/` (reviewable, diffed) or is emitted
  only on demand.
- Whether `effects`/`invariants` stay waived until the manage/effect
  vocabulary lands, or the registry blocks their inclusion.

## Related

- `.plan/api-assurance.md` — enforcement split (S1–S7), consumers and the
  measured coverage profile this task formalizes.
- `.plan/authz-check.md` — the policy artifact and bidirectional gate that
  would supply the `effects` classification.
- `docs/api-assurance.md` — the permanent strategy record; the profile and
  registry become part of it once implemented.
