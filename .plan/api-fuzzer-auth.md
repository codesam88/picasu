---
status: in-progress
type: feature
priority: high
area: testing
---

## Problem

The Schemathesis harness authenticates as a single admin identity, so
generative traffic misses the real threat surface: non-admin, delegated
guest, and anonymous access. All roles need equal run time with a
differential BOLA/BFLA oracle, fed by a well-seeded repo state.

## Goal

Differential per-role fuzzing (option A): one deterministic pass per
role with fixed seed and equal effort, cross-role status/body diff
against a checked-in matrix, fail on new diffs only. Seed guarantees
per-role accounts, per-scope objects, and share/invite variety with
fail-closed liveness.

## Done

- Auth failure diagnosis: static `PICASU_JWT` plus tier-1 no refresh
  plus incomplete poison guards; snapshot bearer never minted in
  tier 1.
- Login-seeding check: `seed.sh` already logins via
  `/post/users/authenticate`; static part is the Schemathesis side.
- Research: `docs/briefs/schemathesis-multi-role-auth.md` (multi-role
  patterns, per-role runs over rotation, refresh bounds, oracles).
- Strategy doc: `docs/api-fuzzing.md` (threat model, A1-A5 plus
  differential, seed principles, equal-effort distribution, gates);
  ops stay in `utils/schemathesis/README.md`.
- Step 2: `utils/schemathesis/seed.sh` extended to full target matrix
  (admin0/admin1, memberA/memberB, sharing-disabled, disabled user;
  global/nested/personal albums; open/nodownload/upload/password/expired/
  member/dead/nested shares; valid/bound/used invites) with fail-closed
  verify. Fresh seed run and `just fuzz-check` pass (0 findings).
- Step 3: per-role runner `utils/schemathesis/run-roles.sh` plus
  `roles.toml` and `hooks_roles.py`, `just fuzz-roles` recipe. One
  deterministic pass per role (admin0, admin1, memberA, memberB,
  sharingDisabled, disabledUser, shareGuest, snapshotBearer, anonymous)
  with identical reduced budget (max-examples=3, deadline=15s, seed=42);
  separate `report-<role>.json`. Reduced run: 9 reports, generated
  counts 1606-1989, no rate/limit errors.
- Step 4: `hooks_roles.py` now does role-aware credential handling
  (session cookie, runtime-minted snapshot bearer, share-guest headers
  with optional password, anonymous stripping), refresh-on-401 with
  attempt/success/failure counters printed at run end (disabled
  credentials exhaust after one failed attempt; forced invalidation
  probe shows 3 attempts/3 successes and self-heal), and poison guards
  covering logout, password/admin/disable/sharing, delete, index
  rebuild, read-only toggle, signing-key reset, config nulling. Reduced
  `just fuzz-roles`: 9 reports, generated counts 1600-1945, no
  rate/limit errors; disabledUser 1 attempt/0 successes/1 failure,
  snapshotBearer 0 attempts.
- Step 5: differential diff plus matrix/baseline wiring.
  `utils/schemathesis/diff.py` loads `report-<role>.json`, groups by
  operation ID, diffs same-operation failure-type outcomes across roles;
  `utils/schemathesis/matrix.json` (per-(role,operation) expected
  class derived from backend/auth-matrix.json) and
  `utils/schemathesis/baseline-roles.json` (4 triaged diffs) shipped.
  `just fuzz-diff` recipe added. Reduced run: 4 findings, all baselined,
  exit 0; deterministic on re-run.
- Fuzzer-found (not fixed, backend out of scope for this step):
  `POST /post/create_dir_album` returns 500 "Failed to create directory:
  File exists (os error 17)" instead of a 4xx when the target directory
  already exists (admin roles, 1 case each).

## Seed target (accepted)

- Users: admin0, admin1, memberA, memberB, sharingDisabled,
  disabledUser, anonymous (no creds).
- Albums/images: global alpha/beta/gamma (mixed minimal/tagged/PNG),
  nested alpha/sub, personal homes for admin0/memberA/memberB,
  trashed and rated items.
- Shares: open, nodownload, upload, password, expired, member,
  dead, nested.
- Invites: valid, bound, used (expired only if mechanism confirmed).
- Exports: per-role JWTs/IDs via `seed.env`; fail-closed liveness.

## Steps

1. Confirm mechanics: expired-invite path, personal-home indexing,
   nested discovery, trash/rate bodies, PNG expectations, `exp:0`
   semantics, dead-share flow, second-admin provisioning.
2. Extend `seed.sh` to the target matrix with fail-closed verify.
3. Per-role runner with equal budget plus first-class anonymous
   pass, separate `report-<role>.json`.
4. Hooks: credential handling, refresh-on-401, poison guards.
5. Differential diff plus matrix/baseline wiring.
6. Gates and CI (local `just fuzz-auth`, nightly artifacts) plus
   docs finalization.

## Mechanics (confirmed 2026-11-17)

- Expired invite: no API past deadline → drop from seed target.
- Personal homes: default users root `<data>/users`; index root personal/auto.
- Nested: global index creates child albums; parentAlbumId set.
- Trash/rate: edit_flags/edit_rating bodies above; timestamp from prefetch.
- PNG: EXIF + XMP, not XMP-only.
- Share exp: 0 never; past accepted at creation.
- Dead share: create then disable sharing.
- Second admin: register then PUT /put/users/admin with X-Picasu-Password.
