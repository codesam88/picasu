---
status: done
type: feature
priority: high
area: backend
---

## Notes

R5 (also F7): session JWT is set client-side via `js-cookie`, readable by any
JS. Move issuance server-side: `POST /post/authenticate` sets
`jwt=...; HttpOnly; SameSite=Strict; Path=/; Max-Age=14d; Secure?<proxy-https>`
and returns the session profile only (no raw token in body). New
`POST /post/logout` clears the cookie (JS cannot clear HttpOnly).

Scope decisions (2026-10-15 interview):

- Only the identity token (`Claims`/Admin) moves to a cookie. Snapshot/asset
  Bearer tokens (`ClaimsTimestamp`/`ClaimsHash` via `Authorization` header +
  service worker/IndexedDB) stay as-is; switching them would break the media
  chain and share-link semantics. `bootstrap`/`register` mint no tokens, so
  only `authenticate` (+ its open-mode branch) changes.
- Cookie-only body (breaking change): authenticate returns
  `{ userId, admin }`, never the JWT string. API clients parsing the body
  must migrate to cookies.
- Frontend identity signal: login response profile stored in memory/Pinia,
  not a new GET endpoint. `useCurrentUser` reads the store.
- `Secure` is conditional: set iff `X-Forwarded-Proto: https` (proxy
  terminates TLS; backend itself is plain HTTP). Default plain-HTTP LAN
  deploys get a non-Secure cookie, mirroring today's client logic. Proxy
  docs must forward the proto header.

## Steps

1. Backend (TDD): authenticate sets HttpOnly cookie + returns profile;
   open-mode branch too; `POST /post/logout` clears cookie; guards unchanged
   (still read `jwt` cookie). Done: `cargo test` auth suites green, new tests
   assert `Set-Cookie: HttpOnly`, no token in body, logout clears.
   (done 2026-10-15: lib 611 green incl. 15 authenticate tests.)
2. Frontend (TDD): `login()` returns profile; session store holds
   `{ userId, admin }` (localStorage-persisted, zod-validated, cleared on
   logout and on any app 401); pages/logout use it; `useCurrentUser` reads
   store; drop `js-cookie` writes of `jwt` (dep removed). Done: vitest 165
   green (20 files), vue-tsc + eslint clean.
3. Docs/spec/e2e: `docs/auth.md`, `auth-flows.md`, `rocket-guards.md` brief,
   OpenAPI annotations + `backend/openapi.json` + `docs/openapi-reference.md`
   regen, `.oasdiff-ignore.txt` entry for the intended 200-body break,
   Playwright cookie-flow updates. Done: openapi-sanity/routes-match/
   json-match/breakcheck green; first-admin-registration + album-tree specs
   green live; interpreter auth scenarios green live.
4. R1 drive-by (prescribed fix, unblocked e2e): `resetEnrollmentProbe` on
   login success and in `performLogout`; RegisterPage already did. Covered
   by source tests + `logout` cache-reset unit test + live spec step 7.
   R1 itself stays open in `auth-enrollment-review.md` for the flow-model
   work to subsume.

## Notes (2026-10-15)

- Interpreter full run: 33 anonymous-media scenarios fail identically with
  and without this change — they navigate app routes with an empty store,
  and the Oct-14 enrollment guard (2ea82b58f) forces `/register`. Positive
  proof: the same album flow with a `user:` given + UI login passes live.
  Those scenarios need a `user:` given (separate task, not this one).
- `backend/tests/api_watcher.rs` harness now sets `bootstrap_admin` (was
  R2-broken); `api_config.rs` compile break stays for the R2 task.
- `.plan/auth-enrollment-review.md` got a prettier-only emphasis-mark
  normalization from `just plan-format` (no semantic change).
