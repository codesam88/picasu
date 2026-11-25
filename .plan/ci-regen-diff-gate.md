---
status: done
type: chore
priority: high
area: repo-tooling
---

## Notes

Replace per-artifact drift gates with one uniform CI mechanism: run
`just docs-regen`, fail on `git diff --exit-code`. Precommit already
auto-fixes, so CI only fires on bypasses. Determinism is proven for all
offline generators; `docs-openapi` (network) and bench extract (run data) stay
out of regen and keep no gate, as today.

Remove from the fail path: `openapi-json-match` + `auth-matrix-check` from the
`openapi-check` chain (delete recipes), `gates-check` drift (keep unit tests
as `gates-test`), quint `doc-check` from `quint::check` (keep recipe for
manual use). Keep all validation gates
(sanity/lint/breakcheck/routes-match/spectral) and the backend
`committed_artifact` test untouched.

## Steps

1. CI `backend` job: install plan, run `just docs-regen`, fail on tracked diff
   with a message naming the fix.
2. Slim the chains (above), rewire `gates-test` into `docs-check.yml`, drop
   the advisory `gates-check` step there.
3. Verify: shellcheck, `docs-check`, `quint check`, full `docs-regen`
   idempotent (second run byte-identical), simulate drift (touch a generated
   file) and show the CI check failing, then restore.
