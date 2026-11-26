#!/usr/bin/env bash
# precommit-md.sh — markdown cleanup flow for precommit.
# Scopes: tracked (staged+unstaged vs HEAD) + untracked *.md under
# .plan/, docs/, utils/, scripts/. Regenerates gates/auth-matrix
# docs when their inputs change, including the vendored-widdershins OpenAPI reference.
# All in-scope markdown is now formatted and staged, including generated docs.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
cd "$root"

# --- (c) plan binary version gate -------------------------------------------
# shellcheck disable=SC1091
source "$(dirname "${BASH_SOURCE[0]}")/deps/versions.env"
if ! command -v plan >/dev/null 2>&1; then
  echo "precommit-md: 'plan' binary not found." >&2
  echo "  Install with: just install plan" >&2
  exit 1
fi
have="$(plan --version 2>/dev/null || true)"
if [[ "$have" != "plan $PLAN_VERSION" ]]; then
  echo "precommit-md: wrong 'plan' version (got '${have}', want 'plan $PLAN_VERSION')." >&2
  echo "  Install with: just install plan" >&2
  exit 1
fi

# --- collect changed files (tracked + untracked scope) -----------------------
changed=()
while IFS= read -r f; do
  [[ -n "$f" ]] && changed+=("$f")
done < <(git diff --name-only HEAD 2>/dev/null || true)

while IFS= read -r f; do
  [[ -n "$f" ]] && changed+=("$f")
done < <(git ls-files --others --exclude-standard -- '*.md' 2>/dev/null || true)

# de-dup (guarded: empty input must stay empty under set -u)
if ((${#changed[@]})); then
  mapfile -t changed < <(printf '%s\n' "${changed[@]}" | sort -u)
fi

in_scope_md() {
  local f="$1"
  case "$f" in
    .plan/*.md) return 0 ;;
    docs/*.md) return 0 ;;
    utils/*.md) return 0 ;;
    scripts/*.md) return 0 ;;
  esac
  return 1
}

# --- (a) regen when inputs changed ------------------------------------------
any_match() {
  # returns 0 if any changed file matches the given extended regex
  local re="$1"
  [[ ${#changed[@]} -eq 0 ]] && return 1
  local f
  for f in "${changed[@]}"; do
    if [[ "$f" =~ $re ]]; then
      return 0
    fi
  done
  return 1
}

regenerated=()
openapi_gen_ran=0

# backend code may affect the spec: regenerate openapi.json first (code -> spec)
if any_match '^backend/'; then
  echo "[precommit-md] regenerating backend/openapi.json (just openapi-gen)"
  just openapi-gen
  openapi_gen_ran=1
  regenerated+=(backend/openapi.json)
fi

# auth-matrix runs when openapi-gen ran OR openapi.json itself changed
if (( openapi_gen_ran )) || any_match '^backend/openapi\.json$'; then
  echo "[precommit-md] regenerating auth-matrix artifacts"
  node scripts/auth-matrix.mjs .
  regenerated+=(docs/auth-matrix.md backend/auth-matrix.json)
fi

if any_match '^(justfile|scripts/quality-gates\.mjs)$' || any_match '^\.github/workflows/'; then
  echo "[precommit-md] regenerating docs/quality-gates.md (just gates-docs)"
  just gates-docs
  regenerated+=(docs/quality-gates.md)
fi

# quint inputs changed -> regenerate docs/eventual-consistency.md
if any_match '^utils/quint/(pipeline|coverage)/' ; then
  echo "[precommit-md] regenerating docs/eventual-consistency.md (just quint docs)"
  just quint docs
  regenerated+=(docs/eventual-consistency.md)
fi

# bench generator inputs changed -> reminder only (needs run data; never auto-run)
if any_match '^utils/bench/(bench|battery|render_baselines|compare_reports)\.py$' ; then
  echo "[precommit-md] reminder: bench generator inputs changed; run 'just bench extract' manually if baselines need refreshing (needs run data)." >&2
fi

# openapi reference regen (widdershins is vendored; runs offline)
if (( openapi_gen_ran )) || any_match '^backend/openapi\.json$'; then
  echo "[precommit-md] regenerating docs/openapi-reference.md (just docs-openapi)"
  just docs-openapi
  regenerated+=(docs/openapi-reference.md)
fi

# single format pass over all markdown (plan78 + docs/utils at 80)
just docs-format

# stage exactly the in-scope hand-written md files that docs-format touched
while IFS= read -r line; do
  [[ -n "$line" ]] || continue
  f="${line:3}"
  case "$f" in *" -> "*) f="${f##* -> }" ;; esac
  if in_scope_md "$f"; then
    git add -- "$f"
  fi
done < <(git status --porcelain)

# Stage regen outputs so the commit contains the refreshed docs, not the
# stale staged blobs.
for f in "${regenerated[@]:-}"; do
  [[ -n "${f:-}" ]] || continue
  if [[ -n "$(git status --porcelain=v1 -- "$f" 2>/dev/null)" ]]; then
    git add -- "$f"
    echo "[precommit-md] staged regenerated $f" >&2
  fi
done

exit 0
