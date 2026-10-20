---
status: backlog
type: perf
priority: medium
area: devops
---

## Problem

Lint/check/test cycles are slow both locally (precommit runs broad gates on
every commit; fresh worktrees pay a cold toolchain build) and in CI
(`checks-n-tests` ~8-10 min per PR, CodeQL Rust ~8.5 min on top, audit +
analyze + review jobs on the critical path). Baseline measured 2026-10-20
during the deps-apply run of PRs 120-123.

## Goal

Reduce wall-clock time for the common paths — a local commit, a PR CI run,
a fresh worktree bootstrap — without losing any gate. Perf work must stay
measure-first: each change carries a before/after number.

## Known facts

### CI jobs and today's timings (PR 120 run, 2026-10-17 wave re-measured)

| job                   | time    | notes                                                           |
| --------------------- | ------- | --------------------------------------------------------------- |
| checks-n-tests        | 10m08s  | green on all three dep PRs; 7m55s for workflow-only PR          |
| audit                 | 2m06s   | failing on pre-existing RUSTSEC set (main-level, not PR-caused) |
| Analyze (rust) CodeQL | 8m39s   | rebuilds backend with CodeQL instrumentation                    |
| Analyze (javascript)  | 55s     |                                                                 |
| Analyze (typescript)  | 59s     |                                                                 |
| dependency-review     | 8s      |                                                                 |
| test-release          | skipped | runs only on main                                               |

### Caching already in place (ci.yml)

- `Swatinem/rust-cache` for the Cargo target dir.
- `setup-node` with `cache: npm`.
- `actions/cache` for Playwright browsers.

### Component inventory and costs

| component                              | fetch/build cost observed                                                                    |
| -------------------------------------- | -------------------------------------------------------------------------------------------- |
| Rust crate graph                       | 169 packages locked per `cargo update` log; cold build not yet measured locally              |
| `cargo install cargo-deny` (audit.yml) | full source install on every audit run — dominant share of the 2m job                        |
| frontend `node_modules`                | 711 packages; warm-cache `npm install` 10s locally                                           |
| ExifTool + ffmpeg                      | apt-installed per CI run, no cache                                                           |
| Playwright browsers                    | cached; install step remains                                                                 |
| plan tool (`just install-plan`)        | installed per CI run                                                                         |
| spectral (`openapi-lint`)              | via npx; fresh checkouts without node_modules fail non-interactively (hit today)             |
| oasdiff binary (`openapi-breakcheck`)  | downloaded per run, pinned by sha                                                            |
| local precommit                        | fmt + clippy + openapi-sanity (66 handlers) + spectral + prettier + plan-lint on each commit |

### Known gaps / levers (candidates, not committed)

- audit.yml: replace source `cargo install cargo-deny` with a prebuilt binary
  (cargo-deny publishes release artifacts; taiki-e install-action supports it).
- Backend tests: plain `cargo test`; cargo-nextest would parallelize and let
  cheap unit suites finish while integration tests build. Also plain
  `cargo test` recompiles fixtures in debug in the same target dir clippy used.
- `just check` + `just test` share one job/target dir already (good); the
  clippy-then-test overlap is real work duplication — measure before touching.
- CodeQL rust 8m39s: scoping queries or accepting it is a cost/benefit decision.
- Fresh worktree pain (observed today): npm/npx spectral fails without
  node_modules; consider `npm ci` bootstrap note or a `just bootstrap` recipe.
- oasdiff/spectral fetches: cache or pin into node_tools.
- Job splitting: checks-n-tests is one monolithic job; backend/frontend/docs
  could be separate high-parallelism jobs (trade-off: billing, cache misses
  per job due to per-job rust-cache keys).

## Steps

1. Measure-first pass: record cold/warm local times for backend cargo test,
   frontend vitest + playwright, clippy-after-build overlap, and a fresh
   worktree bootstrap; record CI run times before/after one representative PR.
2. Apply the cheap wins with clear numbers (audit binary install, node
   tooling pinning).
3. Evaluate cargo-nextest and job splitting with actual numbers; adopt or
   reject with evidence in dated lines below.

## Decisions

- (none yet)
