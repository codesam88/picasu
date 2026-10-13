---
status: backlog
type: chore
priority: medium
area: backend
---

# Settings consistency: tiers, common parsing, enforcement

## Problem

Every setting today picks its own interfaces ad hoc: most are
file+env, four are file-only (`normalize_upload_filenames`,
`validate_upload_content`, `use_client_timestamp_info`, legacy
`password`), one is file+env+CLI (`trivial_passwords`), two are
CLI-only one-shots. Many more detailed gallery settings are coming,
and without tiers each will invent its own coverage — plus docs that
drift from the code.

## Strategy (settled 2026-10-13)

Settings fall into tiers, each with fixed interfaces:

1. **Startup/deployment subset** — frequently used, useful to
   control from the startup environment (address, port, homes, and
   a curated few more; finalize the list during implementation):
   CLI + env + file, parsed once with precedence
   CLI > env > file > default.
2. **Runtime gallery settings** — the detailed settings to come:
   controlled primarily via the API, stored to `config.toml`. No
   CLI twin each; no env twin each.
3. **Debug overrides** — env-only under a `PICASU_DEBUG_*`
   hierarchy, never persisted to the config file or API. Includes
   `--trivial-passwords` (+ `PICASU_TRIVIAL_PASSWORDS`): flag and
   env only, never file/API. Decide during implementation whether
   `PICASU_PBKDF_ITERATIONS` migrates to `PICASU_DEBUG_PBKDF_ITERATIONS`.
4. **One-shot commands** (`--dump-openapi`, `--check-openapi`) —
   CLI-only, special and separate from settings.
5. **Secrets** (`password`, `auth_key`) — never CLI (command lines
   leak via `ps`/history); file + env only.

Mechanism: CLI flags bridge onto their env twins before boot, so
precedence falls out of the single existing application path. A
table test pins the tier assignment (every setting names its
tier; unlisted settings fail) until this graduates into a dylint
check per `api-assurance.md` step 9 — ideally against a single
source array the docs table is generated from.

## Steps (sketch — details at build time)

1. Inventory the current matrix (tiers as found, gaps listed).
2. Assign tiers; move `trivial_passwords` out of file/API.
3. Decide the PBKDF env rename; bridge the startup subset.
4. Table test pinning tier assignment; README/config docs follow.
5. Graduate to dylint enforcement with the api-assurance tooling.

## Leaves out

- New gallery settings themselves (they land in tier 2 by default).
- Runtime toggling of tier-3 flags (by construction not settable).
