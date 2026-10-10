---
status: done
type: bug
priority: high
area: backend
---

## Notes

Remediate the four reproduced authorization defects from the security
assessment (findings F1–F4, recorded verbatim in the appendix below): cases
where a share guest exceeds its own policy or reaches another album. The
appendix is the problem statement and the verified exploit transcripts; this
plan is the code-fix work item.

### Direction (decided)

- **Keep `GuardResult<T>` in handler signatures.** It preserves the
  `impl Responder for AppError` error body and the declared `401`/`405` OpenAPI
  responses. Switching to bare guards plus catchers is out of scope.
- **Consume the witness.** Stop discarding the guard (`let _ = auth?;`). Bind
  `let auth = auth?;` and derive the operation's target from `auth.claims`, so
  the guard value — not the request body — is what authorizes the target.
  `GuardShare::claims` and `GuardTimestamp::claims` already carry the resolved
  share; `GuardAuth` carries the admin role.
- **Security tests pin each finding.** `backend/src/tests/authz.rs` reproduces
  F1–F4 end-to-end through HTTP (share creation, per-token minting) and is the
  regression pin for each step; it must fail before its fix and pass after.
  These are focused reproductions, not the systematic gate: the static rules,
  policy artifact, and negative-test matrix stay in `.plan/authz-check.md`.
- This is the Rocket-idiomatic split for a `Result`-wrapped guard: enforcement
  still happens through the wrapper's `?`, but the handler must use the proof it
  was handed. See `docs/auth.md` §Paradigm and the Rocket request-guard
  "guard transparency" recommendation.

### Gaps addressed (from `docs/auth.md`)

| Step | Finding | Record      | Defect                                                                                                                                                                        |
| ---- | ------- | ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1    | F1      | Appendix F1 | No `typ`/`aud` distinguishes the three token types; a `ClaimsHash` decodes as `ClaimsTimestamp` with `resolvedShareOpt: None`, bypassing metadata/download hiding and renewal |
| 2    | F3      | Appendix F3 | `set_album_title` / `set_user_defined_description` take `GuardShare` but take the target from the body, so any share edits any album                                          |
| 3    | F2      | Appendix F2 | `get-metadata` authorizes "knows the asset ID", not "entitled to this album"                                                                                                  |
| 4    | F4      | Appendix F4 | Renewal is not bound to the presenter's share and does not re-validate the embedded share                                                                                     |

## Steps

### 1 — F1: discriminate the token types

Add an explicit type marker so a token cannot be decoded as another type.

- `backend/src/router/auth.rs`: add a `typ` claim to `Claims` (line 22),
  `ClaimsTimestamp` (line 134) and `ClaimsHash` (line 86); constructors set it.
  Add `#[serde(deny_unknown_fields)]` to each — the three field sets are
  disjoint, so this rejects cross-type decode even before the `typ` check.
- Introduce one typed decode entry point per token type (e.g. a `TokenClaims`
  trait with an expected marker and `decode_typed::<T>`); reject on mismatch.
  Replace the untyped `my_decode_token::<T>` / raw `decode::<T>` call sites:
  `try_jwt_cookie_auth` (line 278), `GuardHash` (500), `GuardHashOriginal`
  (556), `GuardTimestamp` (848), `TimestampGuardModified` (730/752),
  `renew_timestamp_token` (941), `renew_hash_token` (645).
- Migration: tokens minted before this change lack `typ` and are rejected on
  decode — equivalent to the existing `auth_key`-rotation invalidation. A deploy
  forces re-login and a fresh prefetch. Note it in the changelog/commit.

### 2 — F3: bind share-guarded writes to the caller's album

- `backend/src/router/put/edit_album.rs` `set_album_title` (line 250): consume
  `auth.claims.get_share()`; when `Some(share)`, require
  `set_album_title.album_id == share.album_id`, else refuse. Admin (`None`)
  unaffected.
- `backend/src/router/put/edit_description.rs` `set_user_defined_description`
  (line 56): consume the share; after resolving the target `asset_id` from the
  snapshot/index, load the record and require its `album()` equals the share's
  `album_id`, else refuse.
- `Claims::get_share(self)` currently takes `self`; change to `&self` (clone the
  `ResolvedShare`) so handlers can borrow claims without moving them.
- Out of scope here: the related design note that description writes do not
  consult `showMetadata` — that is a policy question, not the F3 target-binding
  defect.

### 3 — F2: album-scope `get-metadata`

- `backend/src/router/get/get_metadata.rs` (line 47): after composing
  `abstract_data`, if the token's `resolved_share_opt` is `Some(share)`, require
  `abstract_data.album() == Some(share.album_id)`, else `404`. Admin (`None`)
  unaffected. This consumes the witness the handler already reads for
  `show_metadata`.

### 4 — F4: bind renewal to the presenter's share and re-validate

- Extract a shared resolver returning the `ResolvedShare` for
  `(album_id, share_id, req)` and re-run `validate_share_access`; use it from
  both `GuardShare` and the renewal handlers (today `resolve_share_internal`
  returns `Claims` and is private).
- `renew_timestamp_token` (line 934): consume the presenter's claims; when the
  presenter is a share, require the submitted token's `resolved_share_opt` share
  to equal the presenter's; re-resolve and re-validate the embedded share from
  the DB before re-issuing. Admin presenters may renew, but the embedded share
  (if any) is still re-resolved and re-validated.
- `renew_hash_token` (line 638): extend `TimestampGuardModified` to carry the
  presenter's `ClaimsTimestamp` (resolved share included) instead of only the
  timestamp; keep the existing timestamp-match check and re-validate the
  presenter's share state before re-issuing.
- Effect: a disabled, expired, or password-changed share stops yielding renewed
  tokens; a leaked timestamp token cannot be kept alive by an unrelated share.

### 5 — Reconcile `docs/auth.md`

- Update §Mechanism (token table, lines 93–103) with the `typ` claim.
- Correct §Paradigm (lines 49–53): enforcement is the `GuardResult` `?`, and
  handlers consume the guard as the authorization witness; "authorization is not
  checked inside handler bodies" is not what the code does.
- Mark F1–F4 resolved in §Findings and refresh §Recommendations.

## Verification

- `backend/src/tests/authz.rs` is the reproduction suite for F1–F4: run
  `cargo test --lib tests::authz`. Each test must fail before its step's fix and
  pass after; a passing-immediately test proves nothing.
- The systematic gate (static rules, policy artifact, token × guard
  cross-product, principal × scope matrix) stays in `.plan/authz-check.md`; this
  plan does not build it. If authz-check is deferred, the focused tests above
  are still required before any step is claimed done.
- Run `just backend-check` and `just backend-test` per step.

## Progress

- 2026-10-09: **Implemented.** F1: `typ` claim + `decode_typed` +
  `deny_unknown_fields` on all three claims types (migration: pre-change tokens
  lack `typ` and are rejected — same effect as an `auth_key` rotation). F3:
  `set_album_title` / `set_user_defined_description` derive the target from
  `GuardShare::claims` (403 on mismatch). F2: `get-metadata` 404s on an asset
  outside the share's album. F4: renewal requires presenter share == token
  share and re-validates the embedded share from the DB; `TimestampGuardModified`
  now carries the full `ClaimsTimestamp`.
- Security tests added in `backend/src/tests/authz.rs` (7 tests) — all failed
  before the fixes (RED) and pass after (GREEN). Verified: `cargo test` (477 lib
  - integration, 0 failures), `just backend-check`, `just openapi-check`
    (`backend/openapi.json` regenerated for the new 403 responses).

## Appendix — remediated finding records (F1–F4)

Moved verbatim from `docs/auth.md` so the record lives with the work. Transcripts describe the pre-fix system.

#### F1 — JWT claim type confusion lets any share bypass its own policy (High, reproduced — fixed)

`ClaimsHash` and `ClaimsTimestamp` are indistinguishable to the decoder: both
are HS256 tokens under the same key, neither carries a `typ` claim, and serde
ignores the extra fields while defaulting the missing `resolvedShareOpt` to
`None` (which `resolve_show_download_and_metadata` treats as full access). A
share viewer already receives `ClaimsHash` tokens in every `get-data` row, so
the attack uses only tokens the share itself is legitimately given:

1. **Metadata hiding bypass.** Calling `GET /get/metadata/<id>` with a
   `ClaimsHash` as bearer instead of the timestamp token returned the full
   `path` object (`/tmp/.../images/albumA/a.jpg`) where the legitimate token
   correctly returned `path: null` for the same `showMetadata=false` share.
2. **Download-hiding bypass.** Calling `GET /get/get-data` with the same
   confused bearer made the server mint fresh tokens with `allowOriginal: true`
   for a `showDownload=false` share; that token then fetched
   `GET /object/imported/...` — HTTP 200, bytes identical to the original file
   (the legitimate token never produces `allowOriginal=true`).
3. **Unlimited renewal.** `POST /post/renew-timestamp-token` accepts a
   `ClaimsHash` as the submitted body token and re-issues it as a
   `ClaimsTimestamp` with `resolvedShareOpt: null`, converting a 300-second
   asset token into a renewable full-policy token.

Snapshot scoping still holds (rows remain limited to the share's album), but
every policy flag a share is supposed to enforce — metadata hiding, download
hiding — is bypassable by the share itself. **Fix:** add a `typ` claim
(`admin` / `snapshot` / `asset`) to each token type and reject mismatches at
decode; optionally decode with `deny_unknown_fields`.

#### F2 — `get-metadata` is not scoped to the share's album (Medium, reproduced — fixed)

A share on AlbumB holding a valid `ClaimsTimestamp` fetched AlbumA's asset by
ID and received its full metadata (path included). Authorization on this route
is effectively "knows a 64-character asset ID" rather than "is entitled to this
album". Asset IDs are high-entropy, but they appear in URLs, logs, and referrer
headers; once known, any share (subject to its own `showMetadata` flag) can
read any asset's metadata. **Fix:** resolve the token's `resolvedShareOpt` and
verify the requested `asset_id` belongs to that album before composing the
record.

#### F3 — Share-guarded write endpoints do not bind the caller to the target (High, reproduced — fixed)

Two routes accept `GuardShare` but then take their target from the request body
without comparing it to the authenticated share's album:

- `PUT /put/set_album_title` — AlbumB's share renamed **AlbumA** (HTTP 200;
  verified by re-reading the album list as admin). Album IDs are visible in
  share URLs (`/share/<albumId>-<shareId>`), so they are not secret between
  guests.
- `PUT /put/set_user_defined_description` — AlbumA's share wrote a description
  into an asset of AlbumB by supplying AlbumB's snapshot `timestamp` (HTTP 200,
  write verified via metadata read). Snapshot timestamps are wall-clock
  milliseconds; combined with the absence of rate limiting (F5), blind
  enumeration of a time window is practical.

Both are integrity violations by any share guest against any album. (Related
design note, documented in the route itself: description writes do not consult
`showMetadata` at all.) **Fix:** derive the target album from
`auth.claims.get_share()` and reject any body `albumId`/snapshot that does not
belong to it.

#### F4 — Token renewal is not bound to the presenter's share (Medium, reproduced — fixed)

`POST /post/renew-timestamp-token` validates that the _presenter_ holds any
valid share (`GuardShare`) and that the submitted token is
signature-valid-but-possibly-expired — but never checks that the submitted
token's embedded share equals the presenter's. Verified: share1's credentials
successfully renewed share2's timestamp token, which was re-issued with
share2's album binding intact. Consequences:

- A leaked timestamp token can be kept alive indefinitely by anyone holding any
  other valid share credential; expiry is only a speed bump.
- Renewal re-issues the embedded share claims **without re-running
  `validate_share_access`**, so a share that has since been disabled, expired,
  or had its password changed continues to yield refreshed tokens as long as
  the token was minted beforehand.

`renew-hash-token` is better bound (presenter must hold a `ClaimsTimestamp`
whose `timestamp` claim matches) but inherits the same missing re-validation on
repeated cycles. **Fix:** compare the submitted token's share against the
presenter's, and re-resolve + re-validate the embedded share at renewal.
