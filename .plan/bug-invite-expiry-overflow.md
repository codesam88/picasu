---
status: done
type: bug
priority: medium
area: backend
---

## Problem

`POST /post/invites` answers 500 for a large positive `expiresInHours`.
`resolve_expiry` (`backend/src/auth/invites.rs`) computes
`now + hours.unsigned_abs() * 3600`; the multiplication overflows `u64`
for hours above ~5.1e15. A debug build panics (→ 500); a release build
wraps silently and stores a nonsensical deadline. Found by the
Schemathesis fuzz check (`utils/schemathesis/baseline.yaml`, entry
`bug-invite-expiry-overflow`).

## Goal

No 500/panic for any `expiresInHours` value. Extreme expiry is rejected
as 400 (the documented client-error class) or clamped to a documented
maximum; release and debug builds agree on the outcome. Spec matches
the chosen behavior.

## Steps

1. Pin the behavior: boundary inputs (max `i64`, hours past the u64
   overflow point, large-but-valid) assert 400 JSON or the clamped
   value — never 5xx.
2. Fix the arithmetic (checked/saturating multiply + explicit bound);
   add `resolve_expiry` boundary unit tests.
3. Update spec annotations if error classes change; `just openapi-gen`
   - `just openapi-check` green.
4. Full backend suite; then remove the `bug-invite-expiry-overflow`
   entry from `utils/schemathesis/baseline.yaml` and confirm
   `just fuzz-check` stays green (finding must be gone, not just
   tolerated).

## Resolution (2026-10-30)

Rejected-as-400, not clamped: `resolve_expiry` enforces
`MAX_EXPIRY_HOURS = 8760` (one year) with `InvalidInput`, and the
multiplication/addition is checked so no input can panic or wrap in any
build profile. Pinned by `resolve_expiry_rejects_beyond_one_year`,
`resolve_expiry_rejects_i64_max_without_panic`, and handler tests
`extreme_expiry_is_400_not_500` / `max_expiry_is_accepted`.
`GET /get/invites/limits` exposes the bound for the admin console, whose
expiry field now parses `<integer><unit>` (`h`/`d`/`m` = 30-day month)
against the fetched bound. Spec (`openapi.json`, auth-matrix) regenerated;
baseline entry removed.
