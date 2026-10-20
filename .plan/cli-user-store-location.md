---
status: in-progress
type: feature
priority: high
area: backend
---

# `picasu-cmd` — explicit user store location

## Problem

`picasu-cmd` resolves the user store through
`picasu::storage::files::get_data_path()` → `resolve_root("PICASU_DATA_HOME", …)`:
env var → legacy `./config.toml` in the **current working directory** →
platform data dir → cwd. Two gaps follow from that:

1. The server resolves its data home as `PICASU_DATA_HOME` >
   `config.toml` `[gallery] data_home` > the same fallback. The CLI never
   reads the config file, so with a configured `data_home` the CLI writes to
   a **second, orphan store** the server never reads (verified: the server
   used `<config data_home>/db/index_v5.redb`, the CLI wrote
   `$XDG_DATA_HOME/picasu/auth/users.redb`).
2. The CLI never says which file it is operating on, and `--help` names no
   location option. A host admin has to guess.

## Goal (owner decisions 2026-10-20)

- Location precedence: `--userdb <PATH>` > `PICASU_DATA_HOME` >
  `config.toml` `[gallery] data_home`. **No platform/cwd fallback**: with
  none of them set the CLI does not know where the store is and refuses to
  run — including `user add`, which would otherwise create a store in a
  guessed location.
- The CLI reports the file it resolved: `Using users.redb at <path>` or
  `Could not find users.redb at <path>; set location via PICASU_DATA_HOME
 or --userdb`.
- A missing store fails the read commands (`list`, `mod`, `del`, `passwd`);
  `add` creates it (first-admin provisioning).

## Implementation steps (iterative, TDD)

**S1 — lib: pure location resolution.** _(done)_
`auth/store_location.rs`: `UserStoreSource` (`Explicit` / `DataHomeEnv` /
`ConfigFile`), `UserStoreLocation { path, source, data_home }`,
`resolve_user_store(explicit, env_home, config_home)` (pure, unit-tested)
and `locate_user_store(explicit)` (reads the env + the config file).
`model/config.rs`: `configured_data_home()` / `configured_trivial_passwords()`
— read `config.toml` without creating files or initializing global config.
`storage/files.rs`: split the pure path resolution out of `resolve_root`
(no directory creation) and add `config_path_readonly()`.

**S2 — CLI: `--userdb` + discovery report.** _(done)_
Global `--userdb <PATH>` flag; resolution failure → exit 1 with a message
naming both location options; discovery line on stderr (stdout stays
machine-readable); missing store → read commands fail, `add` creates it.
The CLI mirrors the server's password policy (`PICASU_TRIVIAL_PASSWORDS`
over the config key) and initializes a logger, so the weakened-hashing and
trivial-password warnings reach the admin.

**S3 — docs.** _(done)_
`docs/admin-cli.md` (new page: the tool, the store, the location rules, the
commands, the environment variables), `docs/auth.md` (store bullet → link
and the corrected lock semantics), `docs/config.md` (storage locations → the
CLI's rule and its missing fallback).

## Adjacent fix

`passwd_store::read`/`list` swallowed every read error, so a store held by
another process looked like an empty store: `user list` printed an empty
table instead of failing. `read_checked`/`list_checked` report the reason;
the CLI uses them.

## Checks

`cargo test` (backend: 747 lib + 15 CLI integration tests),
`just backend-check`, `just plan-lint`, `just docs-check`.
