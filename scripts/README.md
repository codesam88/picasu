# Conventions for this directory
#
# Helper scripts behind `just` recipes. Recipes stay thin wrappers (argument
# parsing, variable defaults); all logic lives here so it is reviewable,
# shellcheck-clean, and testable outside `just`. `lib.sh` holds the shared
# download→verify→unpack→link plumbing sourced by `install-*.sh`.
#
# - `#!/usr/bin/env bash` + `set -euo pipefail` in every script.
# - Pinned tool versions live in `justfile` and are passed as arguments —
#   never hardcoded here.
# - Scripts take paths as arguments, never assume the working directory
#   (recipes run at the justfile directory; be explicit anyway).
