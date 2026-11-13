---
status: open
type: chore
priority: high
area: devops
---

## Problem

Precommit (`just precommit` on branches) fans out to almost the whole
gate on mixed changes — `backend-check` (workspace clippy), full
`openapi-check` (two `cargo run` builds), `utils-check` + `zizmor-check`,
`utils-test`, `frontend-check` (2x `vue-tsc` + eslint), `env-var-check`,
`docs-check` — so a commit pays a large fraction of CI locally, with
none of CI's caches. At the same time branch precommit runs no tests,
so `backend-test` / Playwright / fuzz / dylint / audit failures surface
only after push. Local `just check` / `just test` do not reproduce CI
(`dylint-check` and `fuzz-check` are CI-only), docs-only PRs bypass
`auth-matrix-check` / `env-var-check` via `ci.yml` paths-ignore, and
`docs/openapi-reference.md` has no freshness gate at all.

## Goal

- Branch precommit is a fast lint/formatting baseline (seconds, no
  `cargo run`, no tool installs, no test suites). Heavier verification
  is user-selected through the existing entry points (`just
backend-check`, `backend-test`, `frontend-check`, `frontend-test`,
  `utils-check`, `utils-test`, `openapi-check`, `fuzz-check`, ...).
- CI is the consistent full gate: every PR that touches code runs the
  same `check` + `test` set, including OpenAPI and docs freshness.
- Dependency audit (`cargo deny`, `npm audit`) warns by default and
  only gates when the PR itself changes dependencies (lockfiles /
  manifests), because the ecosystems have a high pre-existing finding
  rate that would otherwise block unrelated work.

## Strategy

Split by cost and signal: keep sub-second format/lint checks on every
commit; move compile-heavy, install-heavy, and suite checks to explicit
invocation and CI. Deduplicate the gate itself (single clippy scope,
single zizmor run, single `--dump-openapi` shared by its consumers)
so CI time is the slower of two parallel jobs, not the sum of
redundant builds. Close the bypasses (docs-lane parity gates,
`openapi-reference.md` decision, audit pathing) rather than adding new
check types.

Rejected: keeping clippy / `vue-tsc` / `openapi-check` in branch
precommit behind path filters — measured warm cost (`just check` ~47s,
`frontend-check` ~37s in `.plan/build-test-performance.md`) violates
the fast budget, and path filters already miss cross-cutting drift.
Rejected: making audit fail on every PR — pre-existing advisories in
npm/crates would block unrelated changes; gating is kept to PRs that
change the dependency set.

Settled 2026-11-05: tracking follows option A — the `justfile` is the
source of truth; a `just gates-check` gate verifies every
check/test recipe is scheduled in its lane and generates
`docs/quality-gates.md` (rule graph, lane coverage, check families;
see step 10).

## Steps

### 1. Deduplicate `just check` (no behavior change)

Scope: `justfile` `backend-check`, `utils-check`, `zizmor-check`,
`openapi-check` sub-recipes, `scripts/openapi-breakcheck.sh`.
Done: `just check` runs one clippy scope set (no workspace-wide +
per-crate double), one zizmor run, one `--dump-openapi` whose output
is shared by `openapi-json-match` / `openapi-breakcheck` /
`openapi-routes-match`. `just check` still passes on a warm tree.
Out: precommit changes, CI wiring, audit changes.

### 2. Shrink branch precommit to the fast baseline

Scope: `justfile` `precommit` recipe only (+ `.githooks/pre-commit`
if messaging changes).
Done: branch precommit runs format/fast-lint only — `cargo fmt
--check` (scoped), `prettier --check` on changed files, `eslint` on
changed files, `plan-lint`, `docs-check` scoped to changed markdown —
no `cargo run`, no installs (`oasdiff`/`zizmor`/`schemathesis`), no
`vue-tsc`, no test suites. `main`-branch precommit keeps full `just
check` + `just test`. `git commit` on a mixed change completes in the
fast budget on a warm tree; heavy checks remain one `just` invocation
away (named in the precommit echo).
Out: `just check`/`test` contents, CI workflows.

### 3. Align `just test` with what CI runs

Scope: `justfile` `test` recipe.
Done: `just test` = `backend-test` + `utils-test` + `frontend-test` +
`fuzz-matrix` + `fuzz-check` (promote tier-1 into the local gate so a
local run reproduces CI). `dylint-check` stays CI-only (nightly
toolchain) and is documented as such in the recipe comment.
Out: precommit, audit, docs gates.

### 4. Make CI consistent, close the docs/contract holes

Scope: `.github/workflows/ci.yml`, `docs-check.yml`, `justfile`
`openapi-check` / `docs-openapi`.
Done: (a) docs-lane runs `auth-matrix-check` + `env-var-check`, not
just `docs-check` + `plan-lint`; (b) `docs/openapi-reference.md` gets
a freshness check in CI or is explicitly declared generated-on-release
(one line in `docs-check.yml` or its removal from the freshness
expectation — decide in the step); (c) `ci.yml` still runs both jobs
for code/contract changes so OpenAPI + docs freshness gate every such
PR.
Out: audit workflow, local precommit.

### 5. Audit warns, gates only on dependency changes

Scope: `.github/workflows/audit.yml`, `justfile` `audit` recipe
comments, `docs/test-strategy.md` tooling caveats.
Done: PR audit keeps its manifest/lockfile path filter (gates only
PRs that change the dependency set); all other runs (scheduled,
unrelated code PRs if ever added) are advisory
(`continue-on-error` + report, no red gate). `test-strategy.md:249`
claim ("every PR") corrected to match.
Out: deny/audit configuration content (advisories, licenses), code
changes to satisfy findings.

### 6. Update the pipeline docs

Scope: `docs/test-strategy.md` (pipeline table, tooling caveats,
known gaps), `AGENTS.md` only if a project rule changed.
Done: the documented pipeline matches the new reality — precommit =
fast baseline, `just check`/`test` = CI reproduction, advisory vs
gating labeled per check, `openapi-reference.md` and
`auth-matrix`/`env-var` lanes stated. `just docs-check`, `just
plan-lint` pass.
Out: code, workflows, recipes.

### 7. Add `gates-check`: justfile is the source of truth (done 2026-11-05)

Scope: new `just gates-check` recipe (+ `scripts/quality-gates.mjs`), `justfile` `check` recipe,
`.github/workflows/ci.yml` + `docs-check.yml` (call it; no inline
tooling), generated `docs/quality-gates.md`.
Done: the script parses the `justfile` recipe graph (check/test
recipes and their dependencies) and the workflow steps +
`.githooks/pre-commit` dispatch, and fails on (a) a `just
<domain>-check|-test` recipe invoked by no lane, (b) a CI step running
tooling without a `just` recipe, (c) generated-doc drift. It writes
`docs/quality-gates.md`: a lane graph (mermaid `graph TD`: job lanes
`precommit`, `ci-backend`, `ci-frontend`, `ci-dylint`, `ci-zizmor`,
`docs`, `audit`, `nightly`, `release` → directly-invoked entry
recipes → transitive recipe deps; recipe nodes labeled `just X`
(including the `just precommit` lane, which is entered by running
it), workflow lanes `ci X`; lane nodes only where edged, lane
ids prefixed so they cannot collide with recipe ids) plus one section per
check/test with the annotation taken from the recipe's comment line
above it in the `justfile` (the `just --list` summary). Check-only by
default (no writes); `node scripts/quality-gates.mjs <root>
--generate <path>` writes the document explicitly (`just gates-docs`
regenerates the committed doc). `just
gates-check` runs inside `just check` and in CI so a new check
such as `shellshock` cannot land unscheduled: adding the recipe
without wiring its lane fails the gate, and the docs regenerate with
it. `just docs-check`, `just plan-lint` pass.
The doc layout described here was replaced in step 10.
Out: lane-semantics changes (steps 1-5 own those), registry files
(option B, not pursued), runtime CI reporting.

### 8. Promote release-only verifications into just recipes (option A, done 2026-11-05)

Scope: `justfile` (new `msrv-check`, `openapi-routes-release`),
`.github/workflows/msrv.yml`, `release.yml`,
`scripts/quality-gates.mjs` isGateRecipe exact list (+ test),
regenerated `docs/quality-gates.md`.
Done: `msrv.yml` runs `just msrv-check` (add the Install-just step;
recipe reads the floor from `backend/Cargo.toml`, installs the minimal
toolchain, `cargo +floor check --workspace --locked --all-targets`);
`release.yml` runs `just openapi-routes-release` (checks the already
built `./target/release/picasu` against the committed spec, same
assume-built contract as `PICASU_BINARY`). `openapi-routes-release`
joins the isGateRecipe exact names so the release lane lists it;
`msrv-check` matches `-check` and lands in the nightly lane. `just
gates-check` passes and the doc is regenerated. No tracker or
pattern-list changes, no workflow-native classification.
Out: CodeQL/docker/packaging (workflow-native mechanics), check-(b)
pattern changes, steps 1-6.

### 9. Track reachability, not names; gate mismatches and orphans (done 2026-11-06)

Scope: `scripts/quality-gates.mjs` (+ test), regenerated
`docs/quality-gates.md`.
Done: tracked universe = transitive callees of `precommit`/`test`/
`check`/`audit` plus transitive callees of `just X` invoked in any
enabled workflow (name heuristic kept only to tell checks apart from
build/install/format helpers, which stay reachable-but-undisplayed).
Lanes gain trigger labels parsed from each workflow's `on:` block
(e.g. `release (tags, manual)`, `nightly (scheduled)`). Lane mapping
is open-world: unknown jobs map to `ci-<job>`, unknown files to their
basename, so new CI jobs are tracked instead of silently dropped. Orphans (a), tests not run in CI (b), PR-only recipes unreachable
from aggregates (c), and inline check tooling outside a `just`
recipe are all flagged; severity lives in the report printing
(`WARN:`, exit 0) and only a stale committed `docs/quality-gates.md`
fails the gate. Seed allowlist (c):
`dylint-check` — nightly toolchain, CI-only by decision). Graph and
sections otherwise unchanged in style. `just gates-check` passes.
Out: steps 1-6, recipe/workflow content changes, new lanes.

### 10. Reshape the generated doc for scanning (done 2026-11-13)

Scope: `scripts/quality-gates.mjs` (+ test), `justfile` `gates-check`,
regenerated `docs/quality-gates.md`.
Done: the doc drops the mermaid lane graph, per-check sections,
line references and underlying-file lists. It renders a mermaid rule
graph (recipe edges only, no lanes) and check families (rule,
description, CI lane, trigger) with allowlist notes; lane triggers are
folded into the family tables, so there is no separate lane section.
The family tables render full width through a scoped style block.
Families are an explicit map in the script; an unmapped gate recipe
lands in `Other` with a warning.
`gates-check` runs the generator's node tests before the drift check,
so the doc's structure is covered by the gate that produces it.
Out: lane derivation, wiring checks, recipe/workflow content.

## Verification

Per step: the recipe(s) or workflow(s) it touches, run for real
(`just precommit` on a scratch change for step 2, `just check` /
affected sub-recipe for steps 1/3/4, `just plan-lint` +
`plan-format --check` for the file itself). Final: `just plan-lint`
passes; diff shows only intended files.
