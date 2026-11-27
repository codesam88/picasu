#!/usr/bin/env bash
# precommit.sh — pre-commit hook implementation (runs as `just precommit`).
# Regenerates derived artifacts and stages them, then runs targeted checks
# per staged area. Regen runs before checks so gates verify fresh artifacts.
set -euo pipefail

# External tools this hook shells out to. Fail fast with the one setup command.
for cmd in git just node plan; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "[ precommit ] missing required tool: $cmd — run: just setup-dev" >&2
        exit 1
    fi
done

# Staged paths that trigger regeneration, and the backend subset that needs
# the full OpenAPI flow instead of the docs-only flow.
regen_paths='^(backend/|utils/|\.plan/|docs/|[^/]+\.md$|justfile$|\.github/workflows/|scripts/)'
backend_paths='^backend/'

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

branch=$(git rev-parse --abbrev-ref HEAD)
staged=$(git diff --cached --name-only)

if [ "$branch" = "main" ]; then
    echo "[ precommit ] On main — full test suite is required to pass."
    just check
    just test
    exit 0
fi

echo "[ precommit ] On '$branch' — format/lint enforced; targeted checks/tests run for changed areas."

# --- regenerate first, so checks verify fresh artifacts ----------------------
if echo "$staged" | grep -qE "$regen_paths"; then
    # ensure the pinned plan release (fast no-op when already installed;
    # installs or fails on mismatch — no version logic duplicated here)
    just install plan > /dev/null
    if echo "$staged" | grep -qE "$backend_paths"; then
        echo "[ precommit ] regenerating OpenAPI artifacts (just openapi-regen)"
        just openapi-regen
    else
        echo "[ precommit ] regenerating docs (just docs-regen)"
        just docs-regen
    fi
    # stage everything regeneration may have touched (no-ops when clean)
    git add -- \
		backend/openapi.json \
		backend/auth-matrix.json \
		docs/auth-matrix.md \
		docs/openapi-reference.md \
		docs/quality-gates.md \
		docs/eventual-consistency.md
    while IFS= read -r line; do
        [[ -n "$line" ]] || continue
        f="${line:3}"
        case "$f" in *" -> "*) f="${f##* -> }" ;; esac
        if in_scope_md "$f"; then
            git add -- "$f"
        fi
    done < <(git status --porcelain)
fi

# --- fast checks: cheap enough to run on every commit, no gating ------------
just docs-check
just env-var-check

# --- targeted checks (slower; gated on changed paths) ------------------------
if echo "$staged" | grep -q '^backend/'; then
    just backend-check
fi
if echo "$staged" | grep -qE '^\.(spectral\.yaml|oasdiff-ignore\.txt)$'; then
    # Root files that configure the openapi gate run the gate they configure.
    just openapi-check
fi
if echo "$staged" | grep -q '^utils/'; then
    just utils-check
fi
if echo "$staged" | grep -q '^utils/quint/'; then
    just quint lint
    just quint check
fi
if echo "$staged" | grep -q '^utils/quint/.*\.qnt$'; then
    just quint test
fi
if echo "$staged" | grep -qE '^(backend|utils)/'; then
    just utils-test
fi
if echo "$staged" | grep -q '^frontend/'; then
    just frontend-check
fi

exit 0
