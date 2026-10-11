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

## Steps (sketch — phases in order, details at build time)

### 1. oasdiff breaking-change gate

Install oasdiff (Go release binary, pinned version) and add a `just`
recipe comparing the freshly generated spec against the committed
`backend/openapi.json`: removed routes, new required inputs, dropped
statuses, and narrowed schemas fail CI unless explicitly accepted. The
`openapi-json-match` phase already guarantees the artifact is current, so
the diff is always meaningful. Baseline is the base branch's committed spec
on PRs.

### 2. Dylint spike + openapi-sanity transition review

Pin a nightly toolchain for lint runs only (stable stays the build
toolchain) and spike the AUTH-2 propagation lint. In parallel, disposition
the 17 existing rules for a full transition:

- **Stay in the `syn` checker:** A1–A7, A9 (annotation shape and doc
  conventions), B1–B5 (route/attribute/signature agreement) — syntactic
  checks where type information adds nothing. A3's tag vocabulary stays a
  data file; A9's fail-closed philosophy must be preserved by any
  replacement.
- **Move to dylint:** P2/P3/P4 (guard outcome and body-error analysis) plus
  the new auth rules (AUTH-2 with type-resolved `GuardResult`,
  AUTH-3 Forward detection, AUTH-11/12 parity) — these gain real precision
  from resolved types and cross-module guard lookup.
- **Borderline:** P1 (return-type reasoning) — start in `syn`, move only if
  the spike shows false positives type info would remove.

Expected outcome is therefore a hybrid, not a full replacement — unless the
spike shows the nightly cost is negligible, in which case consolidate. Either
way the decision is recorded rule by rule, and the dropped tool's scope is
deleted, not left running in parallel.

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
custom hooks (login cookie, share headers). Pilot bounds (agreed):
single-operation runs asserting schema conformance and no-500s. Phase two:
links for the token chains (prefetch→get-data→serve,
create→get→demote) plus the first custom assertions:

1. cross-scope reads refused (share-A token × album-B asset → 404);
2. demoted user's token 401s on next admin use;
3. wrong-`typ` tokens rejected outside their guard;
4. mutating endpoints 405 under read-only (doubles as the F9 regression pin);
5. cross-share renewal refused.

RESTler stays the fallback if the auth-harness customization proves
insufficient.

First/quick pilot: phase one is a full single-operation sweep;
phase two (links + custom assertions) runs under a hard 5-minute
timebox, then reports regardless. Harness lives in scratch
(`/tmp`) — the pilot commits nothing. Findings are triaged only
(bug / harness artifact / spec imprecision); fixes become follow-up
tasks. Detailed fuzzer campaign configuration is a separate later
task, not part of the pilot.

Pilot run (quick): full single-op sweep, 918 cases, 43 failures —
36 spec imprecision (undocumented 422/404/415 framework statuses,
wrong export content-type), 4 harness artifacts, 3 real bugs.
Hard 5-min phase-two box: 9/64 ops reached statefully, custom
assertions A2/A3/A5 PASS, A1 holds (corrected replication), A4
partial (F9 pin not established). Bugs filed separately
(`bug-rebuild-dir-album-cache`); remaining 500s (unknown snapshot
id, create_dir_album EEXIST) and SPA-fallback 200s await triage.
Verdict: advisory gate justified with conditions — contract
reconciliation first (document or normalize the 4xx statuses),
harness hardening (config-write ops excluded, stable authKey),
scoped 5-check oracle. Harness in `/tmp/opencode/schemathesis/`,
nothing committed.

### 5. Fold the above analysis and final settled shape into `docs/test-strategy.md`.

Based on a (assumed to be reviewed) auto-derived openapi documentation, the
above tooling forms the api-level assurance layer next while frontend/backend
e2e functional tests.

## Progress

- Recorded; implementation not started.
- Step 1 done: `just openapi-breakcheck` (pinned oasdiff 1.33.0,
  checksum-verified install, base-branch baseline, ignore file with the one
  accepted break) wired into `openapi-check`; RED verified both directions
  independently.
- Step 2 done: AUTH-2 dylint lint in `utils/dylint-lints`, run through
  upstream cargo-dylint (`just dylint-check`, advisory-only);
  GREEN (0 warnings) and RED (exactly 1 warning) verified independently,
  tree byte-identical. Transition decision: P2/AUTH-3 move to dylint,
  A1–A7/A9/B1–B5/P1 stay in `syn`. Follow-ups: `if let Ok`-without-else
  false negative, quarterly nightly re-pin.
