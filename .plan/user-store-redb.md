---
status: done
type: feature
priority: high
area: backend
---

# User store: `users.redb` substrate + `picasu-cmd` admin CLI

## Superseded (2026-10-22)

The consistency model recorded below still describes a session list in the
index database and a delete that purges sessions. Both are gone:
`.plan/auth-state-model.md` replaces the session list with a token version
in the identity record plus a revocation list, so a delete needs no purge
and the identity lives in exactly one place. The rows below record what
shipped then; the current model is in `docs/auth.md` (§ Auth state model).

## Problem

User identity was split across two stores: the redb `USERS` table (inside
the gallery _index_ DB — conceptually rebuildable from filesystem state)
held `UserRecord { admin, sessions }`, and `auth/passwd.json` held the
password hashes. Provisioning wrote both non-atomically; the JSON write
path (`load → mutate → rename`) was atomic per write but not coherent
across processes; there was no offline provisioning path after the
open-mode removal. Error messages promised a `picasu-admin` CLI that did
not exist.

Two concerns were being conflated and are now separated:

1. **Storage-level correctness** — torn writes, cross-process exclusion,
   non-cooperating writers. Currently hand-rolled.
2. **Application-level lost updates** — two admins read a user, both
   modify, the second silently wins. **Not** fixed by any storage engine,
   lock, or transaction; it needs optimistic concurrency.

## Goal

One dedicated `auth/users.redb` file is the userdb of record
(`username ⇒ {hash?, admin, home}`), administered offline by a new
`picasu-cmd` binary and read/written by the server through a single store
module. Sessions stay in the gallery index redb (temporary, high-churn,
safe to lose). Fresh-start, no migration.

## Status

All slices **landed**. The identity move, the `picasu-cmd` CLI, and the
`picasu-admin` → `picasu-cmd` rename (S1–S3 below) shipped; the store
substrate is **`users.redb`**: the JSON write path (flock/tmp/`.bak`) was
replaced, not run alongside — a feature branch proposes the end state, so
no commit carries a dual-backend store. S4 (redb substrate) and S5
(consistency model recorded in `docs/auth.md` + I5 hardening:
`redeem_invite` writes the identity entry with its hash, so invited
registration is caller-ordered-proof) are done. The CLI slices are
recorded here because the CLI is this plan's deliverable and the store
API it depends on did not change across the substrate swap.

### Substrate decision (owner-approved 2026-10-20)

Three options were compared: `passwd.json` + `flock`, a dedicated
`users.redb`, and SQLite.

- **Neither a lock nor a DB engine fixes the admin lost-update** (M2
  non-promise below). That is application-level and identical across all
  three.
- **Dependency cost:** redb is already linked (four existing `.redb`
  files); a fifth adds no new dependency. SQLite adds a crate plus a C
  library for no benefit at this workload.
- **Correctness surface:** redb removes `flock`/`fsync`/`rename`/`.bak`
  and lock hygiene from our trust boundary, and detects external
  modification (closes the non-cooperating-writer hole). JSON keeps all of
  it on us.
- **`0600` / separate file:** equal across all three — a dedicated file
  takes its own mode. (This was the only apparent JSON advantage; it is
  not one.)
- **Manual inspection:** community `redbcli` browses/edits tables;
  `picasu-cmd` is the supported path. The only real loss versus JSON is
  `$EDITOR`-in-a-pinch repairability.
- **House pattern:** "one redb per concern" already holds
  (`index_v5`, `temp_db`, `cache_db`, `expire_db`).

### CLI surface (owner-approved)

`picasu-cmd user list [user] | add | mod | del | passwd <user>`:

- `add`/`mod` take optional attribute flags (`--home`, `--admin` /
  `--no-admin`, `--password`); without `--password`, prompt interactively
  (no echo, re-type confirm, unix-passwd style). Command-line passwords
  are accepted by design as test plumbing.
- `del` mirrors the zero-admin guard; `--force` overrides; purges the
  user's sessions after a successful entry delete.
- Id/password validation reuses `validate_user_id` + `check_strength`.
- CLI is single-threaded (hashing is CPU-bound); data-home resolution
  matches the server (`PICASU_DATA_HOME` first, no `AppConfig::init`).
- `--trivial-passwords` stays server config/env only; the CLI is always
  strict.

### CLI provenance

The CLI rose out of the enrollment review (`.plan/auth-enrollment-review.md`,
R14): error messages and `docs/openapi-reference.md` promised a
`picasu-admin` provisioning tool that did not exist, and the post-open-mode
server has no online path to enroll a second user — offline store
provisioning is the only scaling path (`.plan/e2e-given-reshaping.md`,
problem stanza 1). The tool is therefore a deliverable of _this_ plan; its
e2e-harness adoption is H1b–H1e of `.plan/e2e-given-reshaping.md`.

## Consistency model (normative; recorded in `docs/auth.md`)

The store is a **file with compare-and-swap semantics**, not a database
with interactive transactions.

- **M1 — Single writer, single critical section.** Every mutation runs
  inside one store-level read-modify-write call. The mutation closure
  reads the state it needs _inside_ that section; no operation spans two.
- **M2 — Last-writer-wins per field, deterministic.** Serialization (M1)
  makes concurrent same-field edits deterministic, not merged: the last
  commit is retained. Non-overlapping field edits converge (both apply).
  Mirrors unix `vipw`: it warns, it does not merge.
- **M3 — One authoritative store per fact.** Identity, hash, `admin`,
  `home` live only in the user store; sessions live only in the gallery
  index redb. No fact is duplicated. Operations touching both (deletion:
  entry + sessions) execute in a fixed, crash-safe order and accept the
  documented intermediate state.
- **M4 — Durable, atomic publication.** A reader never observes a torn
  store; a crash leaves the pre- or post-state, never a partial one. With
  `users.redb` this is the engine's guarantee.

**Explicit non-promises:**

- No lost-update protection for two clients editing the same field from
  stale reads (needs a version/`updated_at` pre-image; not implemented).
- No bounded staleness for a running server's cached view of the store
  (a reload signal, out of scope here).

### Invariants (derive tests from these)

| ID  | Invariant                                                                                                                                                                           | Test kind         |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------- |
| I1  | Concurrent distinct-key writes both survive (no lost update)                                                                                                                        | store concurrency |
| I2  | Concurrent non-overlapping field edits on one user both persist                                                                                                                     | store concurrency |
| I3  | No operation sequence leaves zero admins; refusals are returned errors                                                                                                              | store guard       |
| I4  | `hash: None` (or corrupt store) never verifies a password; `verify` is total                                                                                                        | fail-closed       |
| I5  | Invite redeem: a written entry implies the token was consumed (entry-with-hash lands after the invite commit, in the fixed M3 order); pre-existing id ⇒ conflict + unconsumed token | store/invite      |
| I6  | Kill during a write leaves a parsable store at pre- or post-state                                                                                                                   | store durability  |
| I7  | Deletion removes the entry before purging sessions; a crash never lets a deleted user log in                                                                                        | store ordering    |

## Implementation steps (iterative, TDD; each slice ends green + committed)

The store module's public surface (`provision` / `set_password` /
`set_admin` / `set_home` / `delete` / `read` / `list` / `count` /
`is_empty`) is the seam: server and CLI depend only on it.

- **S4 — `users.redb` substrate (landed).** The store module is redb:
  one table (`users`), key = user id, value = the JSON encoding of
  `UserEntry` (the serde shape stays the single source of entry
  encoding; values stay `redbcli`-inspectable). Every mutation is one
  redb write transaction (M1); the closure's `Err` drops the
  transaction unpublished; commit is the atomic publication (M4). Path
  `auth/users.redb`, `0600`. The engine's file lock is held for the
  process lifetime via a path-keyed handle cache, which excludes
  second processes outright: the CLI is strictly offline and fails
  loudly with "held by another process" while the server runs
  (cross-process pin). Readers never materialize the file (the boot
  gate's `exists()` means "provisioned"). The JSON backend
  (flock/tmp/rename/`.bak`) is deleted, not kept alongside — the branch
  proposes the end state. Store tests: full behavior port + I1/I2
  (concurrency), I4 (`hash: None`, corrupt store fails closed), I6
  (child process SIGKILLed inside an uncommitted write; store parses at
  pre-state), plus `remove_store` (handle eviction for test resets).
  `UserRecord` does not surface in the spec; no oasdiff entry needed.
- **S5 — Model record + I5 hardening (landed).** M1–M4, the explicit
  non-promises, and the test-pinned invariants are recorded in
  `docs/auth.md` under "User store consistency model" (the userdb
  storage narrative lives there — `docs/design.md` does not describe
  the user store). The I5 window closed as far as two files allow:
  `redeem_invite` takes the canonical password and writes the identity
  entry with its hash as its own second store write, so the route can
  no longer skip or reorder the pair. The residual cross-store crash
  window (invite consumed, entry absent) is deliberate and documented —
  it fails safe in the direction where the entry never exists without
  the invite having been consumed; the reverse order would risk a
  login-capable orphan. Pins: one-call login-capable redeem (unit) and
  invite-registration route login (end-to-end).

## CLI slices (S1–S3 — landed)

These shipped on the historical `auth-cli-passwd` branch and are imported
onto this branch (commit "user store: identity in `users.redb`,
`picasu-cmd` admin CLI"), already on the redb substrate — the store API
they depend on is unchanged by the substrate swap. They are recorded here
so this plan is self-contained.

**S1 — identity move.** Separate the identity attributes out of the
gallery index redb so one store owns `{hash, admin, home}`:

- Entry shape: the store value is a `UserEntry` (`#[serde(flatten)]`
  `PasswordHash` + `admin: bool` default + `home: Option<String>` default);
  the credentials field is `Option` — a hash-less entry cannot log in
  (fail-closed). The flattening kept pre-change files parsing as
  password-less defaults.
- Store module (`auth/passwd_store.rs`): one read-modify-write primitive
  owns every mutation (`provision` / `set_password` / `set_admin` /
  `set_home` / `delete`), with lockless `read` / `list` / `count` /
  `is_empty`. `provision_user` inserts hash + admin + home together in
  **one** transaction (removes the documented two-store hazard);
  `set_admin` keeps the zero-admin guard **inside** the transaction.
- redb `UserRecord` slims to the session side only (`sessions`); add
  `purge_sessions(id)` for `user del`.
- Rewire all identity readers: admin guard read, claims refresh,
  authenticate, `set_admin`'s zero-admin sync, harness bootstrap seed
  helpers, and the boot-gate emptiness (now "empty store").
- RED pins: (a) entry carries `admin`/`home` after provisioning;
  (b) `set_password` preserves `admin`/`home`; (c) concurrent RMW from
  two callers both land; (d) zero-admin demote refused within one txn;
  (e) login still verifies against the flattened hash.
- Two review-driven corrections folded in: `user_count`/enrollment
  readers follow the store (not redb), and `redeem_invite` rejects ids
  already present in the store (uniform conflict message, invite left
  unconsumed) so invites cannot re-key seeded accounts.

**S2 — `picasu-cmd` binary.** Workspace binary reusing the lib (clap
derive already a dependency): the `user` subcommand group per the CLI
surface above; `list` reads lockless; `del` = entry delete +
`purge_sessions` + zero-admin guard (`--force` overrides); `passwd` and
`add` without `--password` prompt via stdin (`rpassword`, no echo).
Data-home resolution stays byte-identical with the server
(`PICASU_DATA_HOME`, no full `AppConfig::init`). Integration tests drive
the compiled bin (`CARGO_BIN_EXE_picasu-cmd` — the dash is not converted)
against a temp data home: per-subcommand pins, interactive-stdin pins,
and a cross-process pin.

**S3 — reference rename + docs.** Rename `picasu-admin` →
`picasu-cmd` in code and docs (boot error text, config, bootstrap
messages, `docs/auth.md`, `docs/auth-flows.md`), regenerate the OpenAPI
artifacts, and add the closed-window refusal pin naming the CLI remedy.

**S3 residual (still open) — launcher coupling.** The e2e launcher
currently sets `PICASU_BOOTSTRAP_ADMIN=true` unconditionally; the flag
against a non-empty store refuses boot, so the CLI path pairs with
dropping (or conditioning) the launcher flag. This is owned by **H1b** of
`.plan/e2e-given-reshaping.md` (sequencing note: flip the flag after CLI
provisioning lands). Confirm while doing H1b that `main.rs`'s flag
parsing is still needed for the genuine online-window case before
anything is removed.

## Checks (every slice)

`cargo test` (baseline documented per slice), `just backend-check`,
`just docs-openapi` + `just openapi-check` + `just docs-check` (the store
is not API surface — no wire change expected), `just plan-lint`,
`just frontend-check` + `just frontend-vitest` when API clients move.

## Follow-ups (out of scope, tracked for later)

- **HUP/config reload** — bounded-staleness fix for a running server.
- **Optimistic concurrency** — `updated_at`/version pre-image to turn the
  M2 lost-update non-promise into a detectable conflict.
- **e2e harness adoption** of the CLI — H1b–H1e of
  `.plan/e2e-given-reshaping.md`.
