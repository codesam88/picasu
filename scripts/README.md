# Conventions for this directory

Helper scripts behind `just` recipes. Recipes stay thin wrappers (argument
parsing, variable defaults); all logic lives here so it is reviewable,
shellcheck-clean, and testable outside `just`. Modeling-specific checkers
live in `utils/quint/` instead; this directory holds the repository-wide
scripts and the managed-tool installers.

## Layout

- `deps/versions.env` — single source of truth for external system-tool
  versions (pinned versions + checksums). Sourced by every per-tool script.
- `deps/<tool>.sh` — per-tool ensure/check script (idempotent, no root).
  Each accepts `ensure` (default) or `check`.
- `deps-install.sh` — dispatcher. Runs all `deps/*.sh` (run-parts style) or a
  selected subset via `--tool <name>`; `--check` verifies only.
- `prune-branches.mjs` — reports local merged-into-base, local/remote PR
  merged/closed, local no-exact-PR, upstream-gone, and remote no-PR branches.
  Deletion requires `--apply`; the `--expire-local DAYS`,
  `--expire-closed DAYS`, `--expire-gone DAYS` and
  `--expire-archived DAYS` options override the default cutoffs
  (14, 14, 7 and 30 days). Output starts with a category
  overview (keep/prune counts and the applicable option), followed by
  per-category detail sections. Unmerged local branches are archived under
  `refs/archive/<date>/` first (list with `git for-each-ref refs/archive/`,
  remove with `git update-ref -d <ref>`). Run via `just branches-prune`.

## Conventions

- `#!/usr/bin/env bash` + `set -euo pipefail` in every script.
- Pinned tool versions live in `scripts/deps/versions.env` — never hardcoded
  in scripts or the justfile.
- Scripts take paths as arguments, never assume the working directory
  (recipes run at the justfile directory; be explicit anyway).
- `just install <tool>` is the user-facing entry point; it delegates to
  `scripts/deps-install.sh --tool <tool>`.
