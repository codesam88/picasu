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

| Field         | Meaning                                                                                                                                                                                                         |
| ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `guards`      | handler signature guard types, verbatim                                                                                                                                                                         |
| `credentials` | credential types accepted (`session`, `snapshot`, `asset`, `share`)                                                                                                                                             |
| `admin`       | `admin` (admin-only), `any`, `none`                                                                                                                                                                             |
| `shareScope`  | `album` (target confined to the share's album), `n-a` (no album binding) — always token-derived, never a request parameter                                                                                      |
| `fallback`    | non-guard admission paths the handler falls back to: `registration-invite` (invite token in the body), `allow-registration` (the config knob), `bootstrap-window` (first-admin window while the store is empty) |
| `effects`     | mutation class, exactly one: `read`, `write` (content), `manage` (administrative); empty for public operations                                                                                                  |
| `invariants`  | `authz-model.md` IDs the operation is held to, checked for existence and against the guards                                                                                                                     |

Publicness is derived, not authored: an operation whose handlers carry no
contributing guard is public, and S7 pins that to the document's
`security(())`. The matrix generator writes the derived `public` boolean into
the machine artifact so consumers read one shape.

All 51 annotated handlers carry the key; the 23 SPA page shells are
unannotated by design. `effects` uses the `read`/`write`/`manage` vocabulary;
`invariants` reference a source-verified subset of `authz-model.md` and are
backfilled as the classification is reviewed.

## Correspondence rules

| Rule | Claim checked                                             | Enforced by                                   | Status                      |
| ---- | --------------------------------------------------------- | --------------------------------------------- | --------------------------- |
| S1   | metadata present ⇔ `security(...)` present                | `openapi-sanity`                              | gating                      |
| S2   | `security` schemes ⇔ guard types                          | `openapi-sanity`                              | gating                      |
| S3   | `credentials` ⇔ decoded claims type                       | dylint (`credentials_agreement`, gating)      | implemented; gating in CI   |
| S4   | asset touched is bound to the share's album               | behavioral cross-scope checks, matrix-derived | gating (`just fuzz-matrix`) |
| S5   | read-only gate ⇔ `GuardMutate` + 405 declared             | `openapi-sanity`                              | gating                      |
| S6   | password confirmation ⇔ `GuardReauth` + documented header | `openapi-sanity`                              | gating                      |
| S7   | no contributing guard ⇔ `security(())`                    | `openapi-sanity`                              | gating                      |
| S8   | declared `guards` ⇔ signature guards                      | `openapi-sanity`                              | gating                      |
| S9   | `admin` constraint ⇔ `GuardAdmin`                         | `openapi-sanity`                              | gating                      |
| S10  | `effects` class ⇔ guards                                  | `openapi-sanity`                              | gating                      |
| S11  | waivers name a held rule and a reason                     | `openapi-sanity`                              | gating                      |
| S12  | field set closed; `fallback` vocabulary and placement     | `openapi-sanity`                              | gating                      |

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
   `utils/schemathesis/auth_rules.py`: the share-header injection set,
   the probe each operation carries, wrong-credential token selection, read-only
   exemptions, and the seeded scope pairs. The derivation is pure and
   unit-tested; a matrix change that alters a probe fails the tests
   instead of drifting. Destructive-operation disables stay hand-tuned in
   the TOML configs, pinned to the matrix by a consistency test. Tier 1
   (`just fuzz-check`) gates CI against `utils/schemathesis/baseline.yaml`,
   and the fuzz matrix is rebuilt from the model and executed
   deterministically by `just fuzz-matrix`; the stateful campaign
   (`just fuzz-run`) stays advisory because its phases are time-budgeted.
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
   gate too. The one operation that must stay available while read-only is on
   — lifting the kill-switch — carries a typed `waivers` entry naming
   AUTH-12 and the reason; a waiver that names a rule the operation does not
   hold, states no reason, or suppresses nothing is itself a finding (S11).
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

## Limits and open gaps

The static rules compare the annotation with the handler signature and the
document; they do not inspect handler bodies. A guard that admits more than its
name claims is invisible to them, and behaviour that depends on runtime state is
covered only by the probes below.

- **Behavioural cells not yet pinned.** The generated negative matrix covers
  anonymous, viewer and admin principals. Share credentials need a seeded album
  and share, so those cells stay with the scenario suite. Expired shares on
  ordinary reads (`403`), share-flag effects (`show_metadata=false` redacts
  metadata; `show_download=false` mints asset tokens without original access)
  and share-password refusals (`401`) have no scenario cell yet.
- **`shareScope` has no static agreement rule.** The S4 cross-scope probe pins
  one operation; the remaining album-bound routes rely on behavioural scenarios.
- **Claims are discarded after rejection.** 38 sites do `let _ = auth?`: the
  refusal propagates but the claims are never read. The AUTH-4 triage —
  claims-derived target versus guard-as-gate — is open.
- **No expiry or decode ratchets yet.** `VALIDATION_ALLOW_EXPIRED` inside a
  guard impl, and a JWT decode outside `decode_typed`, are hand-verified facts
  rather than checks.
- **`fallback` is a mechanism marker.** It records that registration accepts an
  invite or the open-registration knob, and that bootstrap accepts while the
  store is empty; it cannot say whether a particular invite or knob state is
  valid.
- **`invariants` are partial.** Operations reference a source-verified subset of
  `authz-model.md`; the file stays hand-maintained until the model can derive it
  back.
- **Share query fallback.** `albumId`/`shareId` query parameters resolve a share
  in `GuardPresenter` but appear in no published security scheme.
- **One bearer scheme covers three credential kinds.** `snapshot`, `asset` and
  share-credential tokens all ride `bearer_token`; the extension's `credentials`
  is authoritative until the schemes split.
- **Coverage strength is unmeasured.** Which contract element a checker covers,
  and whether its expectation is independent of the artifact it checks, is
  tracked in `.plan/api-assurance-coverage.md`; the registry and strength floor
  are open.
- **The stateful campaign is advisory.** Tier 1 (`just fuzz-check`) gates
  against an accepted-findings baseline; `just fuzz-run` is time-budgeted and
  not byte-reproducible.
