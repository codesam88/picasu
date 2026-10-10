---
status: open
type: feature
priority: high
area: backend
---

## Notes

Derive property tests from the model in `docs/authz-model.md` and run them.
This plan builds a **derived regression suite**, not an independent baseline:
model, properties, and implementation share one authorship, so nothing here
catches a misunderstanding common to all three. Genuine independence (an
externally authored oracle or foreign-language reference policy diffed against
the server) is out of scope; the independence this plan does buy is pinned
external attack fixtures (§S0d) treated as ground truth the suite must agree
with.

Hand-written reproductions for the four remediated findings live in
`backend/src/tests/authz.rs`; this is the systematic suite the model exists to
drive, and it is expected to find bugs, not merely pin known ones.

### Decision: invariants, not a reference model

Authorization correctness is a property (an implication over requests), so
`proptest` checks it oracle-free — no second implementation to write, trust,
and keep in sync. The model stays non-executable; only its invariants become
code.

### Scope and exclusions

Behavioral invariants owned here: AUTH-1, AUTH-4…AUTH-10, AUTH-13
(header-only bearer). Static, source-shape invariants stay with the gate in
`.plan/authz-check.md` — except a minimal executable AUTH-2 prerequisite built
in S0c, without which no world green below is trustworthy. Out of scope:
non-authorization security surface (rate limiting, default bind, cookie
flags, plaintext credentials, cache headers), `AUTH-14` (review), and full
concurrency (except the targeted probes in S5b).

### Single authority and ownership

`docs/authz-model.md` is the authority. This plan creates no second manifest:
no JSON policy file, no parallel taxonomy. The machine-readable encoding is
owned by `.plan/authz-check.md` §1. Interface between the plans is the AUTH
ids. Static rules live there; behavioral properties live here; S0c is the only
overlap and it is explicitly a stopgap until the gate's rejection-propagation
rule lands.

### Architecture

- `proptest` is a dev-dependency of `backend`.
- Suite: `backend/src/tests/authz_props.rs`.
- Two levels:
  - **pure** — claims encode/decode, derivation/attenuation, renewal payload.
    Fast, stateless, no HTTP.
  - **world** — one fixed world per test (albums, assets, shares with every flag
    combination), then `TestRunner` generates requests against it. Built once per
    test, not per case: `reset_backend_state` is far too slow per case.
- Shared helpers move to `backend/src/tests/fixtures/authz.rs` so `authz.rs` and
  `authz_props.rs` use one set.

### Property catalogue

| Id   | Invariant (model)                   | Level | Generator              | Assertion                                                                                   |
| ---- | ----------------------------------- | ----- | ---------------------- | ------------------------------------------------------------------------------------------- |
| P-0  | AUTH-2 rejection propagated         | both  | guarded route classes  | unauthenticated/malformed requests are refused; every `GuardResult` binding consumed (`?`)  |
| P-1  | AUTH-1 type soundness               | pure  | claims type            | encode→decode round-trips; a token never decodes as another type                            |
| P-2  | AUTH-10 renewal preserves authority | pure  | claims                 | `renew(t)` has identical payload and bound                                                  |
| P-3  | AUTH-13 header-only bearer          | world | token-bearing endpoint | a token in the query (`?token=`) never authenticates                                        |
| P-4  | AUTH-12 read-only blocks mutation   | world | mutating endpoint      | every mutation answers 405 while read-only is on                                            |
| P-5  | AUTH-3/AUTH-8 original denial       | world | share flags            | an asset token without `allowOriginal` never yields original bytes (not a 200 fall-through) |
| P-6  | AUTH-5 flag soundness               | world | `M`, `D` flags         | `M=false` ⇒ no metadata field; `D=false` ⇒ no original and no `allowOriginal` mint          |
| P-7  | AUTH-6 album scope                  | world | own/other album        | a share never reads or writes another album                                                 |
| P-8  | AUTH-7 snapshot binding             | world | two snapshot ids       | a token for one snapshot is refused for another                                             |
| P-9  | AUTH-9 live identity                | world | expiry                 | a disabled/expired share yields no new identity                                             |
| P-10 | AUTH-4 witness binding              | world | cross-album write      | a cross-album write is refused and leaves the target unchanged                              |
| P-11 | external attack fixtures            | world | pinned transcripts     | each reproduced finding's exploit fails against fixed code                                  |
| P-12 | revoke-vs-serve probe               | world | concurrent revoke      | revocation blocks new identity/renewal while outstanding tokens run to `exp`                |

### Expected findings

Candidates already known to be open and expected to fail:
`POST /post/config/import` under read-only (AUTH-12), `?token=` acceptance
and the original-file `Forward` fall-through (AUTH-13, AUTH-3). Each failing
case is recorded as a finding with its endpoint and the model invariant it
violates.

### Acceptance

- `cargo test --lib tests::authz_props` runs the suite; failing cases are real
  violations, reported not silenced.
- Every invariant in the catalogue has a generator and an assertion; every
  property ships with an executable falsification companion (§S0f) — a property
  without one is not trusted.
- Route × invariant coverage matrix exists with no silent blanks (§S0e).
- P-0 gates S3/S4: no world green is claimed while AUTH-2 is unchecked.
- The suite is fast enough for pre-commit (world built once per test).

## Implementation steps

Each step is independently runnable and leaves the tree green.

### S0 — Scaffolding and shared fixtures

- `proptest` as a backend dev-dependency (done).
- Move the suite helpers (`setup`, `lock`, `create_album`, `create_share`,
  `prefetch_as_share`, `prefetch_as_admin`, `get_data_asset_token`,
  `album_title`) into `backend/src/tests/fixtures/authz.rs`; have
  `tests::authz` use them so both suites share one set.
- Add `tests::authz_props` with one trivial pure property that runs green.
- **Verify:** `cargo test --lib tests::authz` still passes; the trivial property
  runs; no new warnings.

### S0b — Model-vs-assessment review (before S3)

The model was written from the code and may normalize defects as desired.
Before deriving further tests, diff it against `docs/auth.md` findings:

- `read_config ∈ holds(Share)` vs the config-disclosure finding;
- `write_meta` requiring album membership but no `M` flag vs the
  description-write design note;
- renewal re-validation: password re-check scope (presenter-validated vs
  embedded share);
- any other place the model asserts what the assessment calls a defect.

Deliverable: model errata fixed in `docs/authz-model.md` in the same step, so
no property below verifies a known defect. **Verify:** review recorded in
Progress with each item's disposition (model fixed / finding accepted as
intended / finding filed).

### S0c — AUTH-2 prerequisite gate (before S3)

No world green is trustworthy while a handler may ignore its guard. Land the
minimal executable check first, as a stopgap until the gate's rule exists:

- syntactic scan (test, not prose): every `GuardResult<…>` handler binding is
  consumed (`?` or explicit handling); a discarded binding fails;
- behavioral negatives: unauthenticated/malformed requests to one route per
  guarded class are refused.

**Verify:** P-0 green; deliberately discarding a guard in a scratch handler
fails the scan (falsification for the gate itself).

### S0d — Pinned external attack fixtures (P-11)

Import the reproduced findings as ground truth the suite must agree with: the
four remediated transcripts (already in `tests::authz`) plus the still-open
ones relevant here (`import_config` under read-only, `?token=`, original-file
fall-through). Each is a named test asserting the exploit fails. These are the
plan's only externally authored checks. **Verify:** remediated ones green,
open ones red and named.

### S0e — Route × invariant coverage matrix

Generate the mounted-route list from code and cross it with P-0…P-12. Each
cell: covered (property id), deferred (reason + owning plan/step), or N/A
(why the invariant cannot apply). Store as `docs/authz-coverage.md`. No silent
blanks: an unmapped endpoint is an explicit gap, and a new route must update
the matrix (review checklist). **Verify:** matrix committed; every blank has
an owner.

### S0f — Executable falsification rule

Prose discipline is not enforcement. Every property ships with a companion
that proves it can fail:

- a `*_detects_violation` test feeding a hand-built violating input (wrong
  `typ`, cross-album target, expired share, `?token=`, mutation without
  read-only guard) and asserting the property detects it;
- a vacuity counter: the test asserts the generator produced ≥N distinct
  inputs and exercised the path (request count > 0, distinct targets > 1).

**Verify:** companions green; deleting the implementation check turns the
companion red.

### S1 — Pure invariants

- P-1 (AUTH-1): for each claims type, encode→decode round-trips; a token never
  decodes as another type; wrong `typ` is rejected.
- P-2 (AUTH-10): renewal preserves the payload and bound of the token it
  renews.
- **Verify:** green, fast, no HTTP; S0f companions present.

### S2 — World fixture and one passing behavioral property

- A `World` built once per test: two albums with one asset each; shares on album
  A for every `(M,D)` combination plus a share on album B. Extended only by S5b.
- P-8 (AUTH-7) snapshot binding: a token for snapshot `t1` is refused for
  snapshot `t2`.
- **Why first:** a property expected to hold. A red here means the harness is
  wrong, not the code, so the harness is debugged before it is pointed at
  suspected bugs.
- **Verify:** green.

### S3 — Suspected-failing invariants (the payoff)

Add, one at a time, and record each failure as a candidate finding:

- P-3 (AUTH-13): a bearer token supplied as `?token=` never authenticates.
- P-4 (AUTH-12): every mutating endpoint answers 405 while read-only is on —
  including `POST /post/config/import`.
- P-5 (AUTH-3/AUTH-8): an asset token without `allowOriginal` never yields
  original bytes (and never a 200 fall-through).

- **Verify:** run; for each red, confirm it is a real violation (not a test
  bug) and capture the shrunk input. Deliverable: a findings list with endpoint,
  invariant, and observed status.

### S4 — Behavioral invariants expected to hold

Regression pins for the behavior already fixed; each should be green.

- P-6 (AUTH-5) flag soundness: `M=false` ⇒ no metadata field; `D=false` ⇒ no
  original and no `allowOriginal` mint.
- P-7 (AUTH-6) album scope: a share never reads or writes another album.
- P-9 (AUTH-9) live identity: a disabled/expired share yields no new identity.
- P-10 (AUTH-4) witness binding: cross-album writes are refused and leave the
  target unchanged.

- **Verify:** green; S0f companions prove none is vacuous.

### S5 — Triage and disposition

- For each S3 finding: fix inline when small and safe (`GuardReadOnlyMode` on
  `import_config`; drop `?token=`; `Forward` → `Error` in the original guard),
  otherwise open a plan. Record the decision on the finding.
- **Verify:** fixed findings turn green; deferred ones stay red and named.

### S5b — Targeted concurrency probes (P-12)

Small, explicit, not a general concurrency harness: revoke-vs-serve (disable a
share while its outstanding token serves to `exp`, then assert no new identity
and no renewal) and renew-vs-expire. Serial world setup, concurrent request
phase only. **Verify:** probes green; a probe that cannot be made deterministic
is recorded as a gap, not silenced.

### S6 — Integration and traceability

- The suite runs under `cargo test` (lib tests) and `just test`; add a focused
  `just authz-props` recipe for iteration if useful.
- `.plan/authz-check.md` §4 references this suite as its negative-test
  component (it does not duplicate it); the model's derivation table gains the
  suite and matrix references.

## Progress

- S0 (pending): `proptest` added; `fixtures/authz.rs` written; wiring and the
  suite file remain.
