---
status: open
type: feature
priority: high
area: backend
---

## Notes

The working spec for the task: problem, goal, implementation steps. Refine
these sections in place as understanding changes; append only short dated lines
for settled decisions. Keep it concise — progress chatter and back-and-forth
belong in commit messages, not here.

## Problem / purpose

The round-3 critical review (`/tmp/opencode/personal-albums-review-r3.md`:
two discoverers, cross-model challenge, parent mechanism verification)
upheld the isolation model but found its enforcement incomplete: several
mutating and read paths never check the caller, so the per-handler
`write_gate`/`visible_to` convention the feature relies on is not
universal. This task closes the gaps, with tests proving each one.

## Settled decisions

- Scope is the round-3 FIX NOW list only (F1–F10 below); RIDE-ALONG items
  stay batched for when adjacent code is next touched; no new scope.
- F5 (share survives move into personal area): REFUSE moves carrying
  foreign-owned shares. Rationale: the alternatives (prune, reveal, or
  otherwise disable someone else's share) would break expected behavior
  for the share's owner and holders. The move is refused; nothing is
  rewritten; the creator keeps full control of their share.
- Verification: every fix ships with a failing-first test proving a
  second user cannot read/mutate the first user's personal data through
  the fixed path, plus the standard gates (`just check`, backend + frontend
  suites, openapi/auth-matrix where annotations change).

## Open decisions (blocking their item; defaults apply if undecided)

- D1 (F6): export — scope/filter to caller-visible rows, or document an
  explicit operator carve-out? Secret redaction (share URLs, password
  hashes, absolute paths) happens either way.
- D3 (F7): personal_albums_enabled semantics — wire the switch through
  gating/provisioning/scan, or document users_home-alone semantics?
- D4 (F8): MyAlbums — personal-only selector, or document "all visible
  roots" (mirrors Albums global-only)?
- D5: user deletion vs personal tree — purge/quarantine, refuse while data
  exists, or document-and-keep? (carried over; no challenger verdict)

## Implementation steps

Each step is one worker invocation, TDD (RED observed, then GREEN), one
commit per step after parent review. No step starts until the previous
one is committed.

### F1 — Gate all ID/snapshot-addressed writes (S1)

`set_album_title`, `edit_tag`, `edit_rating`, `edit_description`,
`rotate-image`, `regenerate-thumbnail` (+ audit `update_album`'s other
callers): `visible_to`/`write_gate` before any filesystem or DB effect,
cross-user refusal tests (403 for per-asset/by-id, 404 for invisible
albums per canon). Mechanical; same shape as gated `edit_flags`.

### F2 — Input-validity sweep across all routes (S1)

Starting from the known instance (regenerate-thumbnail writes the file
before DB lookup, `&asset_id[0..2]` panics on short/multibyte input):
sweep every route that turns request input into filesystem paths, array
indices, or snapshot lookups; resolve + validate + gate before any
filesystem or DB effect. Fix straightforward cases inline in the
established style; do NOT change anything non-trivial — report it as a
finding with a recommendation instead (non-trivial = unclear ownership
semantics, behavior change, or ambiguous error contract).

### F3 — get_metadata any-id on user sessions (S1)

`visible_to` check on the composed row (or album-scope session tokens).
Practical via share-disclosed ids; add cross-user refusal test.

### F4 — Tag census unfiltered in edit_tag response (S2)

Return caller-filtered `Tree::read_tags`; assert hidden tags absent.
Test-only plus one-line fix.

### F5 — Refuse moves carrying foreign-owned shares (S2, DECIDED: refuse)

In the move path, when the album's `share_list` contains entries whose
`created_by` differs from the caller, refuse the whole move before any
filesystem effect (no partial rename). Tests: cross-user share on public
album, then owner moves album into own personal tree → 4xx and tree
unchanged; owner's own shares still move fine.

### F6 — Export streams every personal row incl. share secrets (S2)

Redact secrets (share URLs, password hashes, absolute paths) unconditionally.
Scope vs carve-out per D1.

### F7 — Switch semantics (S2)

Per D3 decision. If wired: `users_home` set + switch off behaves as
feature-off (no provisioning, no scan, no personal rows); tests pin the
2x2 matrix. If documented: state exactly what the switch does and does
not gate.

### F8 — MyAlbums shows global roots (S2 per separation requirement)

Per D4 decision: personal-only selector, or document. Test the chosen
behavior.

### F9 — E2E honesty bundle (S2, test-only)

Positive controls + route assertions on direct-nav checks; real toggle
on/off assertions (current steps are wait.ms no-ops); justify or drop the
assignment skip comment. No production-code change unless a step fails
for a real reason (then file it as a finding, do not work around it).

### F10 — Correct plan/model done-claims (S2, docs-only)

Where no test exists, the docs must not claim a check: adjust S3 per-op
test claims, AUTH-17..20 "static+test" pins, and the toggle coverage
statement to match what the suite actually pins.

## Explicitly out of scope

- RIDE-ALONG batch from the round-3 review (stale allows/comments, doc
  drift, capture.param, filter/line collapses, source_for whitelist,
  OpenAPI enum, toggle catch/rollback, pathless-gate probes, Outside
  fail-closed, cache-key share identity, VideosPage scope, delete
  UX notes) — batch when adjacent code is next touched.
- Snapshot-token caller binding (reaffirmed canon deferral; value noted
  as raised, no work scheduled).
- Anything not listed above.
