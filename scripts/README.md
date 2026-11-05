# Conventions for this directory

Helper scripts behind `just` recipes. Recipes stay thin wrappers (argument
parsing, variable defaults); all logic lives here so it is reviewable,
shellcheck-clean, and testable outside `just`.

## Layout

- `deps/versions.env` — single source of truth for external system-tool
  versions (pinned versions + checksums). Sourced by every per-tool script.
- `deps/<tool>.sh` — per-tool ensure/check script (idempotent, no root).
  Each accepts `ensure` (default) or `check`.
- `deps-install.sh` — dispatcher. Runs all `deps/*.sh` (run-parts style) or a
  selected subset via `--tool <name>`; `--check` verifies only.

## Conventions

- `#!/usr/bin/env bash` + `set -euo pipefail` in every script.
- Pinned tool versions live in `scripts/deps/versions.env` — never hardcoded
  in scripts or the justfile.
- Scripts take paths as arguments, never assume the working directory
  (recipes run at the justfile directory; be explicit anyway).
- `just install <tool>` is the user-facing entry point; it delegates to
  `scripts/deps-install.sh --tool <tool>`.
