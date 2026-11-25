---
status: done
type: chore
priority: high
area: repo-tooling
---

## Notes

Single markdown cleanup flow: local auto-regen + auto-format behind one
precommit helper. Decisions (2026-11-23): `plan format` owns all hand-written
markdown (`.plan/` at `-w 78`, hand-written `docs/` + root `*.md` +
`utils/**/*.md` at `-w 80 --no-reflow`); the two formatters agree on prose
(verified both directions), so prettier leaves markdown entirely (stays for
frontend code). Generated files (`docs/quality-gates.md`,
`docs/openapi-reference.md`, `docs/auth-matrix.md`) are excluded from every
formatter — generator bytes are final — and `gates-check` leaves the
`just check` / CI fail path (advisory, on-demand via `just gates-docs`).

## Steps

1. Strip embedded prettier passes from `scripts/quality-gates.mjs`
   (`formatFile`) and `scripts/auth-matrix.mjs` (post-write block).
2. Add `scripts/precommit-md.sh`: union of staged/unstaged/untracked `*.md`
   under scope; regen `gates-docs` / auth-matrix only when their inputs
   changed (never `docs-openapi` — network); route each changed file to
   `plan format` with per-scope flags; generated files are never formatted
   (warn if touched without input change).
3. Wire justfile: `docs-regen` aggregate, `docs-format`/`docs-check` over
   explicit hand-written sets, precommit calls the helper, `gates-check` out
   of `just check`.
4. Bulk reformat (own commit): `plan format -w 78` on `.plan/`,
   `plan format -w 80 --no-reflow` on hand-written docs/root/utils md.
5. Fix CI (`just plan-lint` no longer exists), update `AGENTS.md`, delete dead
   `scripts/plan-lint.mjs`, regen + commit generated docs.
6. Verify: shellcheck, `docs-check` green, precommit dry-run on scratch change
   (format applied, tree green, scratch removed).
