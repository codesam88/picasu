---
status: in-progress
type: feature
priority: high
area: backend
---

# Auth state model — stateless capabilities + revocation lists

## Problem

The server memorizes auth state it does not need:

- The index DB's `USERS` table holds a **live session list** per user, and
  every guarded request asks "is this jti live?" (`router/auth.rs:917`).
  The list exists so the admin UI could show active sessions — a use case
  that does not survive contact with reality: unused devices are covered by
  the 14-day token expiry, and a suspected compromise is covered by a
  password change (or "revoke all sessions"), not by a device inventory.
- Every outstanding **invite** is stored (hash-keyed), and redemption scans
  the table to enforce single use.
- Every **share** is stored in the album record's `share_list`, even though
  the share capability already travels inside the token
  (`Role::Share(ResolvedShare)`).

All three are _revocation_ problems wearing the costume of _memorization_.
The paradigm in `docs/auth.md` already claims a stateless capability-token
architecture; the mechanism section describes the opposite.

The store split is half-done too: `users.redb` holds the identity record,
but the index DB still holds a second, partial copy of identity (which ids
exist) — `get_user` is used as the existence gate by the login, the admin
listing and the password-change route, so a user provisioned offline (by
`picasu-cmd`) gets 401 at login, is missing from `GET /get/users`, and gets
404 on a password change.

## Goal (owner decisions 2026-10-20)

- **Stateless capabilities.** A token carries everything needed to use it.
  The server remembers **revocations only**, until they expire.
- **Two files, two lifetimes.**
  - `auth/users.redb` — persistent: the identity record
    `UserRecord {hash?, admin, home, ver}`.
  - `db/index_v5.redb` — ephemeral: the revocation list.
- **`ver` is a per-user token version in `users.redb`.** Bumped by a
  password change, an admin-role change, or an explicit invalidation; the
  token carries the version it was minted with and the guard requires an
  exact match. The CLI can bump it, so `picasu-cmd` can invalidate tokens
  without touching the server.
- **The signing secret is the global epoch.** Rotating it invalidates every
  token at once (the signature no longer verifies); the `ver` counters may be
  reset alongside it, but need not be — they are per-record, not per-secret.
- **No live session list, no stored invites, no stored share list.** All
  three become revocation entries.

## The guard (identity token)

1. Decode + validate: signature (HS256 only), `exp`.
2. Read the identity record: absent → 401.
3. `claims.ver != record.ver` → 401 (the token predates a password or role
   change).
4. `session:<jti>` in the revocation list → 401 (this device logged out).
5. The admin flag comes from the record: a demotion takes effect at once, a
   promotion bumps `ver`, so the old token dies and the role appears at the
   next login.

## Writes

| Event                    | Write                                           |
| ------------------------ | ----------------------------------------------- |
| login                    | none — mint the token with the record's `ver`   |
| logout                   | revoke `session:<jti>` until the token's `exp`  |
| password change          | bump `ver` in `users.redb`                      |
| admin-role change        | bump `ver`                                      |
| invalidate all tokens    | bump `ver`                                      |
| user delete              | delete the identity row — no user, no access    |
| invite redemption/revoke | revoke `invite:<id>` until the invite's expiry  |
| share revoke             | revoke `share:<id>` until the share's expiry    |
| secret rotation          | none — the new signature invalidates everything |

## Implementation steps (TDD, one slice per commit)

**S1 — legacy artifacts.** _(done)_ Rename `auth/passwd_store.rs` →
`auth/user_store.rs`, `PasswdFile`/`PasswdStore` → `UserStore`, `UserEntry`
→ `UserRecord`, the index DB's `UserRecord` → `SessionBundle`, table
`users` → `sessions`, `storage::db::USERS` → `SESSIONS`, store values
`&[u8]` → `&str` (JSON text, one encoding rule).

**S2 — stateless identity.** _(done)_ `ver` in the identity record; the
token carries it; the guard checks it; the index DB's session table becomes
the revocation table (`auth/revocations.rs`); login writes nothing; logout
revokes; the password change and the role change bump `ver`; the existence
checks read the store, so a user provisioned offline can log in;
`GET /get/sessions` goes.

**S3 — CLI token control.** _(done)_ `passwd` and `mod` bump `ver`; a new
`user invalidate <user>` bumps it alone; `user del` no longer purges
sessions (there are none) — `--userdb` alone becomes a complete operation.

**S4 — stateless invites.** The invite token becomes self-describing
(signed); redemption and revocation write revocation entries; the stored
invite records go; `GET /get/invites` goes.

**S5 — stateless shares.** The album record's `share_list` goes; a revoked
share is a revocation entry.

**S6 — docs.** `docs/auth.md` (the state model, the guard, the writes),
`docs/design.md` (persistent record vs. ephemeral index),
`docs/admin-cli.md` (the CLI's token control).

## Checks

`cargo test` (backend), `just backend-check`, `just openapi-check` (route
removals change the wire contract), `just plan-lint`, `just docs-check`.
