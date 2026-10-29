---
status: in-progress
type: chore
priority: medium
area: testing
---

## Problem

Backend and frontend E2E DSLs cover behavior we already state by hand. The
next layer derives assurance from definitions instead of prose: analysis from
the implementation and tests from the contract. The shape is two-sided, with
the OpenAPI document as the single machine-readable requirements artifact in
the middle:

- **White-box (from Rocket handler definitions):** constrain the code to an
  analyzable discipline, then check the discipline with custom Rust lints.
  This derives _analysis_ from the definition.
- **Black-box (from the OpenAPI contract):** generate behavioral tests from
  `openapi.json`, with project invariants encoded as custom checks. This
  derives _tests_ from the definition.

`openapi.json` — already generated from utoipa annotations and already
parity-checked against mounted routes — becomes the authority both sides
read. No prose in the loop.

## Side A — analysis derived from handler definitions

What exists today (`openapi-sanity`) parses handler source with `syn`
(text-level). It works, but it has no type information and breaks on
unfamiliar syntax — which is why every rule carries handwritten scope limits.
The upgrade is custom Rust lints via Dylint (Trail of Bits, full rustc
HIR/type info). The static invariants are exactly lint-shaped:

| Invariant                      | Lint (decidable, syntactic)                                                                              |
| ------------------------------ | -------------------------------------------------------------------------------------------------------- |
| AUTH-2 propagation             | every `GuardResult<T>` binding is consumed (`?` or handled) — with type resolution, no heuristic parsing |
| AUTH-3 Error-not-Forward       | no `Outcome::Forward` in `FromRequest` impls (allowlist recorded exceptions)                             |
| AUTH-11/12 parity/completeness | route attribute + resolved guard set must match the declared classification                              |

Cost, stated honestly: Dylint tracks nightly rustc internals, so it adds
toolchain pinning and maintenance. The cheaper intermediate step is
strengthening the existing `syn` checker; the disciplines it enforces (typed
guards, `GuardResult`, declared annotations) are _what make_ the code
analyzable either way. Keep the code stylized — arbitrary handler bodies are
not statically decidable, and no tool changes that.

What does not fit: full functional verification of handlers. Creusot/Prusti/
Verus need annotation-heavy contracts and fight the Rocket/redb/serde
ecosystem at every step — disproportionate here. And Kani has a precise
limitation that rules out most of the backend: no multithreading, no atomics,
no async runtimes, so Rocket handlers and the redb statics are unreachable.
Kani's real scope is extracted pure functions (`decode_typed`/typ checks,
trim canonicalization, permission-ordering cores split from their DB shells):
`#[kani::proof]` harnesses proving the decision logic over all inputs, with
the I/O shell covered by tests. Cheap where it applies, useless elsewhere —
adopt only alongside refactors that extract pure cores anyway, never as a
general strategy.

## Side B — tests derived from the OpenAPI definition

Schemathesis (active, Hypothesis-backed, OpenAPI 3.1, CI-ready) is the primary
candidate:

- **Single-operation generation (immediate value):** feed it `openapi.json`
  against a seeded test instance; it generates edge-case inputs per operation
  and checks schema conformance plus no-500s. This alone would probe every
  `401/403/404/405` branch handwritten YAML never names.
- **Stateful sequences (the real prize):** via OpenAPI `links` (or explicit
  config), it chains prefetch→get-data→serve, create→get→demote — exactly the
  multi-step choreography where the recorded findings lived. Handwritten YAML
  cannot cover that space; generation can.
- **Project invariants as custom checks:** Schemathesis's built-in oracle only
  finds crashes and schema violations. AUTH-5/6/7/8 become custom checks over
  responses (e.g. "a share-A token's rows never reference album B",
  "demoted user's token 401s on next use"). Auth needs a small harness: login
  hook injecting the cookie, share-header fixtures. The token choreography is
  _not_ inferable from schema alone — that harness is the irreducible
  handwritten part.

Alternatives: RESTler (Microsoft Research, the original stateful REST fuzzer,
real security-bug track record) if Schemathesis's auth customization proves
insufficient — but it is heavier to configure and slower-moving. EvoMaster is
overkill (JVM, whole-program SBST). For API _evolution_ rigidity rather than
behavior, add oasdiff-style breaking-change gates: fail CI on breaking spec
changes (removed routes, new required fields, dropped statuses) unless
explicitly accepted — this is what makes the spec trustworthy enough to
generate from.

## Caveats

1. **Nothing invents properties.** Generated tests amplify _stated_
   invariants; the AUTH list, the custom checks, and the seed/auth harness
   are human-authored. Anyone selling "fully derived assurance" is selling
   the oracle problem back to you.
2. **The spec must be precise first, or generation probes nothing.** If
   `openapi.json` under-specifies (missing 403s, vague schemas), Schemathesis
   cannot aim at those branches. Under this regime the annotation discipline
   matters _more_, not less — a virtuous loop with the existing P-rules.
3. **Absence-of-effect and races stay handwritten.** "Target unchanged after
   refused write" needs follow-up reads (doable in custom checks, awkward);
   TOCTOU/concurrency is out of scope for all of these tools.
4. **Second-source-of-truth risk, inverted.** Today the risk is prose
   drifting from code. With spec-derived testing the risk is the _spec_
   drifting from intent — mitigated by oasdiff plus the annotation checks
   already run.

## Status overview (single source of status; bodies below hold design detail)

| Item                                                                                    | Status                                                                                           |
| --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| Steps 1–5 (breakcheck, hybrid lints, pilot, reconciliation, strategy record)            | done (see Progress)                                                                              |
| Step 6 AUTH-3 lint + triage                                                             | lint done; triage done — 2 genuine findings, gating decision open (fix vs finding-baseline)      |
| Step 6 AUTH-11/12                                                                       | open, blocked on `effects` vocabulary                                                            |
| Step 6 decode agreement                                                                 | done (`decode_agreement` crate)                                                                  |
| Step 6 claims-consumption (F3)                                                          | open, queued                                                                                     |
| Step 7 UI fixtures                                                                      | done (4 crates); UI-suite CI wiring open; allowlist/baseline open                                |
| Step 8 expiry/decode ratchets                                                           | open                                                                                             |
| Step 9 env-var lint                                                                     | done, gating; sequencing sentence needs amendment (open decision); further domains open          |
| Step 10 + S-rules                                                                       | S1/S2/S5/S6/S7 gating, S1 paired both ways, S3 gating (`token_typ_agreement`); done              |
| Dylint gating story (allowlist/baseline, CI wiring, re-pin as run-the-suites)           | open (allowlist/baseline); CI wiring done, S3 gated in CI                                        |
| Consumers: matrix generator                                                             | done (`scripts/auth-matrix.mjs` → human + machine, `auth-matrix-check` phase of `openapi-check`) |
| Consumers: hook derivation, generated negatives (incl. S4), effects/invariants backfill | open                                                                                             |
| `authz-model.md` derivation-back                                                        | deferred until matrix proves itself                                                              |

## Steps (sketch — phases in order, details at build time)

### 1. oasdiff breaking-change gate

Install oasdiff (Go release binary, pinned version) and add a `just`
recipe comparing the freshly generated spec against the committed
`backend/openapi.json`: removed routes, new required inputs, dropped
statuses, and narrowed schemas fail CI unless explicitly accepted. The
`openapi-json-match` phase already guarantees the artifact is current, so
the diff is always meaningful. Baseline is the base branch's committed spec
on PRs.

### 2. Dylint lint + openapi-sanity transition (done)

Background: `docs/briefs/dylint.md`. Pinned a nightly toolchain for
lint runs only (stable stays the build toolchain) and built the AUTH-2
propagation lint with full type resolution. The 17 existing rules
dispositioned for the transition:

- **Stay in the `syn` checker:** A1–A7, A9 (annotation shape and doc
  conventions), B1–B5 (route/attribute/signature agreement), P1, P3,
  P4 — syntactic checks where type information adds nothing. A3's tag
  vocabulary stays a data file; A9's fail-closed philosophy must be
  preserved by any replacement. P1 stays unless false positives show
  type info would remove them; P3/P4 move deferred to a follow-up.
- **Moved to dylint:** AUTH-2 with type-resolved `GuardResult`, and
  the P2/AUTH-3 decision recorded — these gain real precision from
  resolved types and cross-module guard lookup.

Outcome is therefore a hybrid, not a full replacement. Marker
(stable-toolchain lint API) evaluated and dropped: experimental, and
unproven whether its AST exposes the resolved-alias + typeck +
parent-node classification AUTH-2 needs; revisit only if nightly
pinning becomes painful.

### 3. Run the lint through cargo-dylint

Run `utils/dylint-lints` through upstream `cargo-dylint`: pin it like
other tools (`just install-dylint`), declare the lint crate in workspace
metadata, and point `just dylint-check` at `cargo dylint --all -- -p
picasu`. Verify GREEN (zero findings) and RED (exactly one warning on a
synthetic violation, tree restored) through `cargo dylint`. The nightly
pin, the lint source, and the quarterly re-pin policy are the load-bearing
parts — the runner is just mechanics.

Step 3 done (`just install-dylint` pinned 6.1.0, workspace metadata,
GREEN/RED re-verified through `cargo dylint`). Follow-up
survey of the upstream general example set (dylint rev `08e00155`, tag
v6.1.1): all 8 lints build and run, exactly one finding — `abs_home_path`
on `env!("CARGO_MANIFEST_DIR")` in `default_spec_path()` (dev-only
`--check-openapi` gate, acceptable-with-reason, not a bug). Decision: not
wired in — the set needs its own nightly (`nightly-2026-08-20`) plus
`dylint-link`, which doubles the toolchain/tool surface for zero real
findings. `just dylint-check` stays custom-lint-only; re-survey at the
quarterly re-pin if the nightlies converge.

### 4. Schemathesis pilot

Background: `docs/briefs/schemathesis.md`. Python 3.14 is present (no
pip — use `uvx` or a pinned venv). Target is a locally run backend
seeded over HTTP (bootstrap admin first, then fixture albums); auth via
custom hooks (login cookie, share headers). Design: phase one is
single-operation runs asserting schema conformance and no-500s; phase
two is links for the token chains (prefetch→get-data→serve,
create→get→demote) plus the first custom assertions:

1. cross-scope reads refused (share-A token × album-B asset → 404);
2. demoted user's token 401s on next admin use;
3. wrong-`typ` tokens rejected outside their guard;
4. mutating endpoints 405 under read-only (doubles as the F9 regression pin);
5. cross-share renewal refused.

RESTler stays the fallback if the auth-harness customization proves
insufficient.

Agreed bounds (first/quick pilot): phase one is a full
single-operation sweep; phase two runs under a hard 5-minute timebox,
then reports regardless. Harness lives in scratch (`/tmp`) — the
pilot commits nothing. Findings are triaged only (bug / harness
artifact / spec imprecision); fixes become follow-up tasks. Detailed
fuzzer campaign configuration is a separate later task, not part of
the pilot.

Outcome (pilot run): full single-op sweep, 918 cases, 43 failures —
36 spec imprecision (undocumented 422/404/415 framework statuses,
wrong export content-type), 4 harness artifacts, 3 real bugs.
Hard 5-min phase-two box: 9/64 ops reached statefully, custom
assertions A2/A3/A5 PASS, A1 holds (corrected replication), A4
partial (F9 pin not established). Bugs filed separately
(`bug-rebuild-dir-album-cache`); remaining 500s (unknown snapshot
id, create_dir_album EEXIST) and SPA-fallback 200s await triage.
Verdict: advisory gate justified with conditions — contract
reconciliation first (DONE 2026-10-26: `spec-reconciliation-4xx` landed,
exit sweep clean of spec noise) (document or normalize the 4xx statuses),
harness hardening (config-write ops excluded, stable authKey),
scoped 5-check oracle. Harness in `/tmp/opencode/schemathesis/`,
nothing committed.

### 5. Fold the settled shape into `docs/test-strategy.md`.

Record: the 5-check oracle scope (schema conformance + no-500s;
acceptance checks stay off until the spec claims them), advisory status
of both runners (`just dylint-check`, `just schemathesis-run`) with
their gating preconditions (contract reconciliation for the fuzzer,
zero-finding discipline for the lint), the quarterly re-pin cadence,
and `docs/briefs/` as the tool knowledge layer. Done when
test-strategy describes the api-level assurance layer next to the
frontend/backend e2e suites with no stale claims.

### 6. AUTH-3/11/12 dylint spikes

Dylint cannot be called a success on one lint: the remaining
statically-decidable AUTH properties must catch real mistakes or the
runner is ceremony. Definitions in `docs/authz-model.md`
(AUTH-3 denial-vs-forward, AUTH-11 admin isolation, AUTH-12
read-only completeness); background in `docs/briefs/dylint.md`.

- **AUTH-3 (directly decidable):** flag every `Outcome::Forward` in a
  `FromRequest` impl. Exactly 3 sites exist today, all in
  `router/auth.rs` — triage each against the definition (denial
  re-enterable as a weaker route vs legitimate route-variant
  selection); record route-variant exceptions or file bugs. GREEN on
  the tree as-is only if all 3 triage as exceptions; RED via a
  synthetic denial-as-forward.
- **AUTH-11/12 (classification gap):** first resolve G(r) — the guard
  type set per route from signatures — precisely. Then face the gap:
  no machine-readable classification says which routes are `manage`
  or have effects (`security(...)` unused, POST serves reads too, no
  policy artifact yet). The spike either implements the check against
  the minimal declaration source it finds, or reports the guard
  matrix plus a concrete declaration proposal (extend annotation vs
  new attribute) for a follow-up. No hardcoded method/name
  heuristics land in the tree.
- Each lint: GREEN/RED verified, README catalog entry, advisory-only
  (no gating change). P2/AUTH-3 follow-up in step 2 closes when the
  AUTH-3 lint lands.
- **Decode agreement (AUTH-1/F1 ratchet):** each guard decodes exactly
  the claims type the model assigns it (verified by hand: user guards
  → `ClaimsSession`, hash guards → `ClaimsAsset`, timestamp guards →
  `ClaimsSnapshot`). Directly decidable, GREEN on day one — pins the
  F1 incident class.
- **ClaimsSession consumption (F3, queued after):** 23 handlers do
  `let _ = auth?` — rejection propagated but claims never read.
  Extends AUTH-2's use analysis with the target-source question
  (claims-derived target vs body/query-derived on a share-guarded
  write); triage may yield genuine F3s.
- Status: AUTH-3 lint and triage done; the 2 genuine findings need the
  baseline-vs-fix decision (see Status overview) before anything
  dylint gates. AUTH-11/12 waits on the `effects` vocabulary.
  Decode agreement done; F3 still queued.

### 7. UI-test infrastructure for the lint crates

Every lint here is verified by manual GREEN/RED transcripts, which
evaporate at the next re-pin. Port the Clippy workflow: per-lint UI
fixtures (pass + fail) with blessed expected output, wired to `cargo
test` in each lint crate (Dylint ships `ui_test` on the
`compiletest_rs` pattern for this). Open spike question first:
whether `ui_test` cooperates with the manual multi-lint registration
on the pinned nightly. Done when all three lints carry committed
fixtures and the re-pin procedure is "run the suites", not
"re-prove by hand". Status: fixtures committed for all four lint
crates (AUTH-2, AUTH-3, decode agreement, token-typ); the UI-suite
runner wiring (recipe + CI) is open, folded into the dylint gating
story above (CI currently runs the lints over the backend, not the
crates' own UI suites).

### 8. Expiry-confinement and decode-path ratchets

Two GREEN-day-one lints, one worker: flag `VALIDATION_ALLOW_EXPIRED`
inside any `FromRequest` impl (verified: the only production uses
are the two renewal handlers — guards never accept expired tokens),
and flag any JWT decode outside `decode_typed` (verified: zero raw
decodes — the `typ` backstop holds iff every decode flows through
it). Same enclosing-impl + call-site shape as AUTH-3. Span-origin
ignoring (`from_expansion` first line of defense against generated
code) rides along as a drive-by in the same change. Exception
lists stay per-site `#[allow]` until a legitimate exception
appears — no `dylint.toml` mechanism with zero entries.

### 9. Cross-domain consistency lints (queued — third priority)

Same enforcement shape, new domains, after the auth prototype and
the API-assurance core land. First candidate (noted 2026-10-13):
configuration consistency — enumerate every `std::env::var` lookup
in `backend/src` and match the set against the documented `PICASU_*`
variables (`docs/config.md` + README table), failing on undocumented
additions or documented-but-unread removals. Stronger variant: a
single source array (name, type, default, doc) from which the docs
table is generated, with the lint checking lookups against the
array instead of prose. Both kill the same drift class the
`openapi.json` parity gate kills for routes. Further domains follow
the same recipe: pick a property with a machine-readable declaration
source, write the lint, pin GREEN/RED. No new lint starts until the
auth set above is gating. Status: the env-var lint shipped as a node
gate ahead of that sequence — the sentence needs amendment to
dylint-scope (or a recorded supersession) before it can be trusted
as the decision record; further domains stay open.

### 10. Machine-readable auth metadata (spike: `x-picasu-auth` extension)

Per-operation authorization semantics that OpenAPI `security` cannot
express (token types, inline admin role, share scope/binding,
explicit-public) ride in the annotation as a vendor
extension next to the native `security(...)` clause:

```rust
security(("cookie_jwt" = [])),
extensions(("x-picasu-auth" = json!({
    "guards": ["GuardAdmin"],
    "tokenTyp": ["session"],
    "admin": "admin",
    "shareScope": "n-a",
    "public": false,
    "effects": [],
    "invariants": ["AUTH-2"]
}))),
```

Field semantics: `guards` mirrors the handler signature;
`tokenTyp` names the credential types (`session` for the cookie
session token, `snapshot`/`asset` for bearers, `share` for share
credentials); `admin` records the role constraint (`admin`,
`any`, `none`); `shareScope` the enforced album binding (`album`,
or `n-a` when the operation binds nothing by album — always derived
from the presented token or identity, never from a request parameter);
`public: true` pairs with
`security: []`; `effects` the manage/effect classification AUTH-11/12
needs; `invariants` the `authz-model.md` IDs for traceability.
Readonly and reauth behavior are derived from `guards`
(`GuardMutate`, `GuardReauth`), not stored.
Background: `docs/briefs/openapi-auth-metadata.md`,
`docs/briefs/auth-policy-engines.md`,
`docs/briefs/schemathesis-modeling.md`.

Interfaces: `openapi-sanity` parses the extension (A9 no longer
fires), validates shape against one schema, and cross-checks
extension against guard signature (`GuardPresenter` present ⇒ share
schemes in `security`; `GuardMutate` ⇒ 405 declared;
`public: true` ⇔ `security: []`); a generated per-operation auth
matrix (op × principals × typ × scope × readonly × reauth × effects)
is diffed in CI next to the spec for security review; the
Schemathesis tier-2 hooks consume the matrix for share-guarded sets,
typ-aware token selection, scope pairs, and the destructive-op
disable list instead of hand lists; Spectral ignores the key;
generators read it via `vendorExtensions` if an SDK ever needs it.

Success monitoring: annotation coverage (ops carrying the key over
74, tracked per run); guard↔spec agreement (mismatches found by the
new rules, must converge to zero, each mismatch is a bug in code or
annotation); regeneration stability (`openapi.json` + matrix
byte-identical on re-run); Schemathesis auth auto-inference coverage
(share/type-aware probes parameterized by the model, not fixtures);
review sign-off on the generated matrix. The spike passes when one
simple operation carries the key end-to-end (annotation → committed
spec → checker rule GREEN/RED → matrix row) with no gate
regression. Done when all 74 ops are annotated and the agreement
rules gate.

### 11. Check-enforcement split + consumer rollout (design record)

The S1..S7 correspondence rules split by what each tool can see, so
each rule lives where its inputs live and nowhere else:

| Rule | Problem / purpose                                                                                                                                | Enforcement                                                                     |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------- |
| S1   | metadata without a published declaration                                                                                                         | openapi-sanity (done, gating; paired both ways, internal probes exempt)         |
| S2   | `security` schemes vs guard types in signature                                                                                                   | openapi-sanity (done, gating)                                                   |
| S3   | `tokenTyp` vs guard-decoded claims types                                                                                                         | dylint: `token_typ_agreement` crate (done, gating via `just dylint-check` (CI)) |
| S4   | asset touched is bound to the share's album (prefetch filter, renewal binding, 404-on-mismatch; serving drops out — bearer authorizes the asset) | behavioral: generated cross-scope probes                                        |
| S5   | `readonly` vs `GuardMutate` + 405 declared                                                                                                       | openapi-sanity: P1/P2 shape                                                     |
| S6   | `reauth` vs `GuardReauth` + documented header                                                                                                    | openapi-sanity: signature + annotation text                                     |
| S7   | `public` vs `security(())` + no guard                                                                                                            | openapi-sanity: signature + annotation form                                     |

Split justification: annotation/signature comparisons are syn's home
turf; type/call-graph questions are dylint's; binding semantics are
behavioral. Forcing all seven into one tool would re-implement
attribute parsing in HIR or hardcode call tables in syn. The
dylint gating story (allowlist mechanism for triaged findings, CI
wiring, re-pin as `run the suites`) is prerequisite for S3 gating
anything.

Consumers, in build order:

1. **Matrix generator** — per-op auth table from the committed spec,
   diffed in CI; doubles as the security-review baseline. Its docs
   output may feed widdershins into the final API reference; exact
   shape (human table vs machine JSON vs both) decided at build time
   for human as well as tool consumption.
2. **Schemathesis hook generation** — share-guarded sets, typ-aware
   token selection, scope pairs, destructive-op disables derived from
   the matrix instead of hand lists.
3. **Test generation** — token×guard cross-product negatives with
   expected 401/403/404 per cell.
4. **`effects` vocabulary + `invariants` backfill** — controlled
   manage/effect classification (AUTH-11/12), then link ops to it;
   `authz-model.md` stays the prose authority until the
   annotation-based model can derive its docs back, at which point
   the model replaces the hand-maintained file.

Relation to the rest of this plan: steps 1–5 are done foundations
(contract gate, hybrid lint runner, pilot, reconciliation, strategy
record); steps 6–9 stay open (AUTH-3/11/12 spikes, UI-fixture
runner, expiry/decode ratchets, cross-domain lints) and remain part
of the overall strategy. Final shape and reasoning land in
`docs/api-assurance.md` (permanent record for developers and
security reviewers); this file stays the implementation roadmap.

## Progress

- Step 1 done: `just openapi-breakcheck` (pinned oasdiff 1.33.0,
  checksum-verified install, base-branch baseline, ignore file with the one
  accepted break) wired into `openapi-check`; RED verified both directions
  independently.
- Step 2 done: AUTH-2 dylint lint in `utils/dylint-lints`, run through
  upstream cargo-dylint (`just dylint-check`, now gating in CI);
  GREEN (0 warnings) and RED (exactly 1 warning) verified independently,
  tree byte-identical. P3/P4 move deferred; open follow-ups are the
  `if let Ok`-without-else false negative and the quarterly nightly
  re-pin.
- Step 3 done (see step body; includes the example-set survey decision).
- Step 4 pilot run done (see step body; F0 filed as
  `bug-rebuild-dir-album-cache`, remaining findings filed as
  `bug-snapshot-unknown-500`, `bug-spa-fallback-missing-param`,
  `bug-create-dir-album-eexist`, `spec-reconciliation-4xx`,
  `schemathesis-campaign-config`).
- Step 5 done: `docs/test-strategy.md` records the settled api-level
  assurance layer (5-check oracle scope, advisory runners with gating
  preconditions, quarterly re-pin cadence, `docs/briefs/` knowledge
  layer) next to the e2e suites; settled contract facts (parameter
  descriptions, registered security schemes) stated with no stale
  claims.
- Step 10 spike open: `x-picasu-auth` extension plan recorded above;
  success is annotation coverage over 74 ops, guard↔spec agreement at
  zero mismatches, regeneration stability, and model-parameterized
  Schemathesis probes.
- Step 10 rollout done 2026-10-28 (`spike/x-picasu-auth`): all 51
  utoipa-annotated handlers carry the key (23 unannotated page shells
  excluded by design); every annotated op has `security` (public trio
  explicit via `security(())`); S1 pairing rule gates the tree;
  `openapi-check` green with 74/74 routes matched. Open follow-ups:
  generated auth matrix diffed in CI, `effects` controlled vocabulary
  (all `[]` for now), `invariants` backfill once the manage/effect
  classification lands, query-param share fallback documentation.
- Step 11 rules S2/S5/S6/S7 done 2026-10-28 (`spike/x-picasu-auth`):
  S2 security-guard correspondence, S5 readonly-405, S6 reauth-header
  docs, S7 public-explicit boundary — each with fixture RED/GREEN
  pair, count pins, silent real-tree gate, committed separately. S1
  paired both ways (internal-tagged probes exempt). S3
  (`token_typ_agreement` dylint, gating in CI) done 2026-10-28 with UI
  fixtures and clean-tree/RED verification. S4 (behavioral
  cross-scope probes) remains open; S3 is now gated in CI.
