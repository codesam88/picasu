---
status: done
type: feature
priority: high
area: backend
---

## Notes

Completed 2026-11-01: S1–S6 all committed on
`feature/personal-user-albums` (569b70c9, 13e3721b, dd32244f, 8f53b075,
07b497b8, 4b046e8b). Full gates green at the S6 commit: nextest
backend suite 915/915, frontend vitest 45 files/351 tests, fuzz-matrix
26/26, openapi/auth-matrix/env-var/docs checks.

The working spec for the task: problem, goal, implementation steps. Refine
these sections in place as understanding changes; append only short dated lines
for settled decisions. Keep it concise — progress chatter and back-and-forth
belong in commit messages, not here.

## Problem / purpose

A share's bearer token does double duty: it is the map key of the
`share_list` entry server-side and the anonymous-access credential, and it
is served verbatim in album listings (`ShareInfo.url`, built in
`router/get/get_list.rs`), so live credentials travel alongside ordinary
album metadata into browsers, logs, caches, and the unfiltered export
stream. The creator needs the token once (to hand out the link); nobody —
including admins and the export — needs it in a listing. Split identity
from credential: listings carry only a non-secret fingerprint, and the
bearer is retrievable exactly once per need through a creator-only
endpoint.

## Settled decisions

- No migration and no backwards-compatible shims (user-decided
  2026-11-01): the bearer stays as the server-side map key and anonymous
  credential; every wire surface that carries it except the create
  response and the new reveal endpoint switches to the fingerprint in
  the same change. Coordinated frontend + test + fuzz + docs updates
  land together; nothing is deprecated, everything is replaced.
- Token handling everywhere else is unchanged: anonymous prefetch,
  serving, renewal, and share-link URLs keep using the bearer;
  `edit_share` stays creator-only; `delete_share` stays
  creator-or-admin.
- Fingerprint algorithm: blake3 of the bearer token, first 16 hex
  characters (64-bit collision resistance; non-reversible). Same
  primitive the codebase already uses for content hashes.

## Rejected alternatives

- Server-assigned surrogate id: requires rewriting every stored share
  row (migration) for no gain over a handle derived from existing rows.
- Positional or descriptive addressing (nth share, creator +
  description): unstable under concurrent edits, or non-unique.
- Keep serving the bearer to the creator in listings: preserves a
  per-role contract (bearers for creators, fingerprints for others)
  where every present and future listing consumer must get the role
  check right; the uniform no-bearer-in-listings rule is one invariant
  instead of N correct checks.

## Consumer inventory (verified 2026-11-01; worker re-verifies first)

Bearer-as-key or bearer-on-the-wire today:

- Backend: `create_share` mints and returns it once
  (`router/post/create_share.rs`); `edit_share` matches entries by
  `share.url` (`router/put/edit_share.rs`); `delete_share` matches by
  `share_id` (= url); share resolution from headers/query
  (`try_resolve_share_from_headers/query` in `router/auth.rs`);
  renewal + share re-validation compare `share.url`
  (`router/auth.rs`); `ShareInfo.url` copies it verbatim
  (`router/get/get_list.rs`); `get_export` streams raw `Share`
  records (`router/get/get_export.rs`).
- Frontend: `ShareManagement.vue` and `LinksPage.vue` key entries by
  `share.url` and build `${origin}/share/${albumId}-${url}` links with
  copy-to-clipboard; `EditShareModal.vue` carries `url` through the
  edit form and keys the client-side map by it; `CreateShareModal.vue`
  shows the once-returned link; `shareRoute.ts`, `serviceWorker.ts`,
  and `toImgWorker.ts` consume the bearer from opened share links
  (holder path — unchanged by this plan).
- Tests and harnesses: `listed_share_urls` fixture
  (`tests/share_ownership.rs`), authz share helpers, backend_api
  scenario share flows, the fuzz harnesses (`fuzz_matrix.py`,
  `hooks_campaign.py`), and `docs/scenario-dsl.md` all handle raw
  share ids/URLs and move to fingerprints (holder flows keep the
  create-response bearer) in the same change.

## Implementation steps

Each step is one worker invocation, TDD (RED observed, then GREEN), one
commit per step after parent review. Backend first, so the invariant
holds before consumers. No step starts until the previous one is
committed.

### S1 — Fingerprint helper + helper unit tests

`model/album.rs`: `Share::fingerprint()` returning the 16-char blake3
handle; unit tests pin determinism, non-reversibility shape (16 hex
chars), and distinct inputs giving distinct handles. No callers yet.
Done: `cargo test --lib album`, fmt, clippy.

### S2 — Reveal endpoint (creator-only token retrieval)

New `GET /get/share/token?<albumId>&<fingerprint>`: `GuardUser`
session, resolves the album (invisible album → 404, same no-oracle
canon as the share endpoints), looks the entry up by fingerprint,
returns the bearer only when `created_by` equals the caller (else
404); logs retrieval without the token value. OpenAPI annotation with
`credentials: ["session"]`, admin not exempt (admins never receive
bearers). Tests: creator reveals own share → 200 + bearer resolves
anonymously; another user and an admin → 404 with no leak; unknown
fingerprint → 404. Frontend untouched. Done: targeted backend tests,
fmt, clippy, `just openapi-json-match`, `just auth-matrix-check`.

### S3 — Edit and revoke by fingerprint

`edit_share` matches entries by fingerprint (body carries the
fingerprint; stored `url`/`created_by` preserved server-side; creator
check unchanged, still 403 otherwise). `delete_share` accepts the
fingerprint (creator or admin — the admin revoke right — else the
existing 403/404 behavior). Anonymous and creator-bearer flows
untouched. Tests: update existing share_ownership/authz share tests
to address by fingerprint; RED first on the new addressing (old
url-as-id bodies rejected); creator edit/delete green, foreign edit
403, admin revoke of foreign share green, admin edit of foreign share
still 403. Done: `cargo test --lib share_ownership authz`,
fmt, clippy.

### S4 — Listings carry fingerprints, admins see all shares censored

`ShareInfo.url` removed; additive `keyFingerprint` field instead (no
dual-serve period per the no-migration decision). `get-albums`
`shareList`: creators see own entries (fingerprint, no bearer);
admins see every entry with fingerprints only; non-creator
non-admins keep seeing nothing (S4 isolation unchanged). Export
(`get_export`) emits the same censored view admins see, with
passwords and absolute paths redacted regardless of caller.
Tests: creator listing has fingerprint and no bearer-shaped value;
admin listing shows foreign shares with fingerprints only;
viewer listing unchanged; export contains no bearer/password
material. Rewrite `listings_scope_shares_to_their_creator` to the new
contract. Done: targeted suites, fmt, clippy,
`just openapi-json-match`, `just auth-matrix-check`; regenerate
`backend/openapi.json` and auth-matrix artifacts in the change.

### S5 — Frontend share flows on fingerprints + reveal

`ShareManagement.vue`/`LinksPage.vue`: key entries by fingerprint;
link building and copy move behind an explicit reveal action calling
S2 (result held in memory only, never persisted); censored entries
show metadata plus a Revoke affordance calling S3 delete. `Create`
flow unchanged (one-time bearer from the create response).
`EditShareModal.vue` addresses by fingerprint. Done:
`just frontend-check`, `just frontend-vitest` (update specs touching
share URLs), e2e share flows green if covered.

### S6 — Harnesses, scenarios, docs, full gates

Update `listed_share_urls`-style fixtures (fingerprints for
management assertions; create-response bearers for holder flows),
backend_api scenario share steps, fuzz harnesses, and
`docs/scenario-dsl.md` share examples to the new addressing.
Document the invariant (listings never carry bearers; reveal is
creator-only and logged) alongside the F6 export rule and the
AUTH-model entries the new endpoint's annotation touches.
Done: `cargo test --lib backend_api share_ownership authz`,
`just fuzz-matrix`, `just frontend-check`, `just frontend-vitest`,
full `just backend-test`, openapi/auth-matrix match.

## Explicitly out of scope

- Step-up authentication (password re-entry) for reveal: noted as a
  possible later hardening, not this plan.
- Changing anonymous share access, share creation policy, token TTLs,
  or the bearer format itself.
- Public-link password flows beyond the existing `has_password`
  presence flag.
