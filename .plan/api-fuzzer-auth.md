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
