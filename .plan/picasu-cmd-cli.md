---
status: done
type: feature
priority: high
area: backend
---

# `picasu-cmd` — offline user administration CLI

## Superseded (2026-10-20)

The substrate decided below (passwd.json + flock) was revisited:
`.plan/user-store-redb.md` moves the userdb to a dedicated
`auth/users.redb` (redb already a dependency; the CLI's store API is
unchanged, so the CLI surface and S1–S3 below stand as delivered). That
plan carries the consistency model and the current storage decision;
the history below records what shipped first.

## Problem

The user store is split: the redb `USERS` table (inside the gallery _index_
DB, which is conceptually rebuildable from filesystem state) holds
`UserRecord { admin, sessions }`, and `auth/passwd.json` holds only the
password hashes. `provision_user` performs both writes non-atomically; the
`passwd.json` write path (`load → mutate → rename`) is atomic per write but
not coherent across processes; no offline provisioning path exists post
open-mode removal.

## Goal (owner decisions 2026-10-20, confirmed after the step-back review)

`passwd.json` becomes the userdb of record, unix passwd/chfn style:
`username ⇒ {hash, admin, home, …}` — small, human-readable, `0600`,
fsync-before-rename durable, sidecar `.bak` for manual recovery. Sessions
stay in redb (temporary, the only high-churn data, safe to lose). redb
keeps purely rebuildable/temporary state. Fresh-start, no migration.

- Concurrency: advisory flock on a sibling `<passwd>.lock` guards every
  read-modify-write; readers stay lockless (rename publishes whole-file
  snapshots). Shared lib code so server and CLI inherit correctness.
- `--home` is stored (serde optional) now; server consumption belongs to
  the owning plans. `user del` mirrors the zero-admin guard, `--force`
  overrides. Attribute reads eventually cacheable; not in scope now.
- CLI surface: `picasu-cmd user list|add|mod|del|passwd <user>`;
  `add`/`mod` take optional attribute flags (`--home`, `--admin`,
  `--password`); without `--password` prompt interactively (no echo,
  re-type confirm, unix-passwd style). Id/password validation reuses
  `validate_user_id` + `check_strength`; CLI stays single-threaded
  (hashing is CPU-bound).

## Implementation steps (iterative, TDD; each slice ends green + committed)

**S1 — Separate attributes out of redb into passwd.json (identity move).**
The largest slice, committed as one green state because every redb
identity reader must switch together:

- Entry shape: extend the passwd store value to a `UserEntry`
  (`#[serde(flatten)]` PasswordHash + `admin: bool` default + `home:
Option<String>` default), so pre-change files keep parsing as
  password-less defaults — free read-compat, still fail-closed on login.
- New `auth/passwd_store.rs` (or equivalent): the RMW primitive — open
  `<passwd>.lock`, flock exclusive, load, mutate, fsync tmp, rename,
  write `.bak`. All identity ops run inside it:
  `provision_user` becomes ONE locked transaction that inserts hash +
  admin + home together (removes the documented two-store hazard);
  `set_admin` keeps the zero-admin protection _inside_ the locked
  transaction (demote cannot race to a zero-admin store).
- redb `UserRecord` slims to the session side only (`sessions:`
  `Vec<SessionRecord>`); add `purge_sessions(id)` for later `user del`.
- Rewire all identity readers: `router/auth.rs:618` guard admin read,
  `:920` claims refresh, `users_authenticate.rs:289`, `put/users_admin.rs`
  zero-admin sync, harness `tests/bootstrap.rs` seed helpers,
  `bootstrap_open()` emptiness now = empty JSON map (boot gate stays
  existence-based). Boot error text names `picasu-cmd`.
- RED pins first: (a) entry carries `admin`/`home` in passwd.json after
  provisioning (absent today); (b) `set_password` preserves
  `admin`/`home`; (c) concurrent RMW from two coats of the flock helper
  both land (non-deterministic red pre-lock — report honestly); (d)
  zero-admin demote refused within one txn; (e) login still verifies
  against the flattened hash.

**S2 — `picasu-cmd` binary.**
Workspace binary reusing the lib (clap derive already in deps): `user`
subcommand group per the goal; `list` reads lockless; `del` = `user del` +
`purge_sessions` + zero-admin guard (`--force` overrides); `passwd` and
`add` without `--password` prompt via stdin (`rpassword`, no echo).
Data-home resolution stays byte-identical with the server (`PICASU_DATA_HOME`
path used by `get_data_path` — no full `AppConfig::init`). Integration
tests drive the compiled bin (`CARGO_BIN_EXE_*`) against a temp data home:
per-subcommand pins, interactive stdin pins, and a cross-process pin
(CLI subprocess write lands while an in-process thread holds/competes for
the store).

**S3 — Launcher/H1a coupling + docs.**
Drop the e2e launcher's forced `--bootstrap-admin` if present (CLI replaces
that workaround); verify `main.rs` flag parsing still needed for the
online window case. Rewrite the H1a problem stanza in
`.plan/e2e-given-reshaping.md` and the R14 row closure condition in
`.plan/auth-enrollment-review.md` (parent, not workers). Design-doc touch
in `docs/design.md` where the userdb storage is described.

## Checks (every slice)

`cargo test` (732 baseline, delta documented), `just backend-check`,
`just docs-openapi` + `just openapi-check` + `just docs-check` (wire
contract must not change — `UserRecord` schema shrink if a route exposes
it needs a dated `.oasdiff-ignore.txt` entry), `just plan-lint`,
`just frontend-check` + `just frontend-vitest` when `serverErrorSchema`
or API clients are touched; S2 adds the CLI integration suite.
