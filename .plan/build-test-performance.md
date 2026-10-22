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

## Measured baseline (2026-10-22, local: 8 cores, warm caches)

| step                            | time                     | note                                             |
| ------------------------------- | ------------------------ | ------------------------------------------------ |
| `cargo test --lib` (748 tests)  | **152.5 s** (170 s user) | 1.1x parallelism — effectively serial            |
| `cargo test` (all, 775 tests)   | ~160 s                   |                                                  |
| `cargo nextest run` (775 tests) | **29.2 s**               | 5.5x; nextest 0.9.138 already installed          |
| `just frontend-playwright`      | 82 s (42 tests)          | 5-14 s per test; backend started per scenario    |
| `npm test` (vitest, 236)        | 2.7 s                    |                                                  |
| `just frontend-check`           | ~49 s                    | prettier 5 + vue-tsc 13 + vue-tsc 13 + eslint 18 |
| `just docs-check`               | 5.4 s                    | prettier over markdown                           |
| `npm ci`                        | 49 s (11 s CPU)          | network-bound                                    |
| `npm run build`                 | 168 s                    | `npm ci` (49 s) + vue-tsc + vite (16 s)          |
| `just check` (warm)             | 68 s                     |                                                  |
| `just openapi-check` (warm)     | 3.8 s                    |                                                  |

Two findings the numbers name:

- The backend suite runs at ~1.1x parallelism: 170 s of CPU in 152 s of
  wall clock. 218 test call sites take the process-wide `TEST_SERIAL_GUARD`,
  and all of them share one `TEST_ENV` temp data home.
- `frontend/package.json`'s `build` script is `npm ci && vue-tsc --noEmit &&
vite build`. CI installs the dependencies in the workflow and the build
  installs them again: the checks job pays `npm ci` twice.

## Candidates (measured where possible)

| #   | candidate                                    | payoff (local / CI)                        | risk                                                                              |
| --- | -------------------------------------------- | ------------------------------------------ | --------------------------------------------------------------------------------- |
| 1   | `cargo-nextest` for the backend suite        | 160 s -> 29 s / ~2 min off the CI job      | **adopted** — per-test isolation; surfaces tests that leak outside their temp dir |
| 2   | drop `npm ci` from the frontend build script | 49 s / ~50 s off the CI job                | **adopted** — the recipe installs only when missing                               |
| 3   | run the two `vue-tsc` passes concurrently    | 13 s / ~13 s                               | **adopted** — both passes' exit codes are checked                                 |
| 4   | `cargo-deny` as a prebuilt binary            | — / audit job 2m06s -> ~40 s               | **adopted** — pinned prebuilt release                                             |
| 5   | prebuilt/cache the `plan` tool               | — / ~1 min per job that installs it        | **adopted** — the lint gate is local now                                          |
| 6   | spectral as local tooling, not `npx`         | — / a fetch per run + a fresh-checkout gap | pinned version                                                                    |
| 7   | split `checks-n-tests` into parallel jobs    | — / wall = max instead of sum              | CI minutes, per-job setup                                                         |
| 8   | one shared backend per playwright worker     | part of the 82 s                           | test isolation                                                                    |
| 9   | Biome instead of Prettier (+ ESLint)         | ~25 s / ~4 % of the CI job                 | see below                                                                         |

### Biome, measured and assessed

Biome would replace prettier (5 s frontend + 5.4 s docs) and eslint (18 s):
~28 s -> ~3 s, i.e. ~25 s of a ~10 min CI job, and ~23 s of the local
frontend loop. Against that:

- **Biome does not format markdown** (its markdown support is experimental),
  so the docs/`.plan` formatting and the docs-check workflow's `npm ci` stay
  on prettier. Biome would cover the frontend files only.
- The repo lints with `typescript-eslint`'s **type-checked** rule sets
  (`strictTypeChecked` + `stylisticTypeChecked`, ~90 rules that need the TS
  program) plus `eslint-plugin-vue`'s strongly-recommended set. Biome has no
  type-aware linting, so those rules would be lost. Keeping ESLint
  alongside Biome reduces the saving to prettier's ~10 s.
- Biome's formatter is not byte-compatible with prettier's, so the
  migration is a large reformat diff to review.

Verdict: not a CI-time win worth its cost. It is a "one tool for format and
lint" consolidation with a real lint-coverage risk; if it is ever done, keep
ESLint for the type-aware rules, use Biome only as the frontend formatter,
and keep prettier for markdown.

## Adopted (2026-10-22)

| #   | change                                                        | before | after |
| --- | ------------------------------------------------------------- | ------ | ----- |
| 1   | backend suite under `cargo-nextest` (one process per test)    | 160 s  | 34 s  |
| 2   | the frontend build no longer runs `npm ci` itself             | 168 s  | 16 s  |
| 3   | the two `vue-tsc` passes in `frontend-check` run concurrently | 49 s   | 37 s  |

`just test` end to end: ~250 s -> 108 s. `just check`: 68 s -> 47 s.
`just frontend-build`: 168 s -> 16 s (the `npm ci` the CI workflow had
already run was repeated inside the build script).

Notes:

- nextest is installed from the pinned prebuilt release
  (`just install-nextest`, `scripts/install-nextest.sh`, sha256 per platform),
  not compiled from source — `cargo install cargo-nextest` takes minutes.
  CI installs the same pinned binary in the two jobs that run the backend
  suite.
- nextest does not run doctests, so `backend-test` runs `cargo test --doc`
  after it. The doctest count is zero today; the step keeps the gate.
- nextest's per-process isolation surfaced two tests that wrote into the
  platform data home instead of their own temp dir (they skipped the
  harness): `cargo test` hid that by running the whole suite in one
  process. Both now take the harness, which gives every test a private
  data home. This is the "no gate lost" part of the switch: nextest is
  stricter, not looser.
- The frontend build installs the dependencies only when `node_modules` is
  missing, so CI pays `npm ci` once and a fresh clone still works.

## Adopted (2026-10-22, second pass)

| #   | change                                                             | before                 | after         |
| --- | ------------------------------------------------------------------ | ---------------------- | ------------- |
| 4   | `cargo-deny` from the pinned prebuilt release, not `cargo install` | minutes per run        | 1.5 s locally |
| 5   | the plan tool out of the CI gates                                  | a source build per job | no install    |

- `scripts/install-cargo-deny.sh` installs the pinned release binary
  (sha256 per platform) the way the nextest installer does; the audit job
  calls `just install-cargo-deny` instead of `cargo install cargo-deny`.
- The plan tool is the board view only. The frontmatter gate moved into the
  repo: `scripts/plan-lint.mjs` validates every `.plan/*.md` against
  `.plan/.schema.json` with the same rules the tool used (required fields,
  enum values, unknown keys as warnings), and `just plan-lint` calls it.
  CI no longer installs the tool, and `just setup-dev` does not either —
  `just install-plan` is there for the board.
- The audit job is red on a pre-existing advisory set (a RUSTSEC finding
  in `rustls-webpki` and the npm `follow-redirects` advisory). The lockfile
  bumps that clear it land on a separate branch; this branch only makes the
  job cheap enough to keep running.

## Adopted (2026-10-22, third pass): the CI job split

`checks-n-tests` was one job that ran every gate in sequence, so its wall
clock was the sum of the cargo work and the node work. It is now two jobs
that run in parallel:

| job        | owns                                                                      | needs                                                                               |
| ---------- | ------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `backend`  | fmt + clippy, the backend and utils suites, the API contract check        | cargo, node (the embed-frontend build embeds the assets), nextest, ExifTool, ffmpeg |
| `frontend` | the frontend gates, vitest, the end-to-end suite, the docs and plan gates | node, playwright, ffmpeg, the backend binary                                        |

The frontend job runs the end-to-end suite against the binary the backend
job built and uploads as an artifact, instead of compiling the backend a
second time (a cold debug build of the whole crate).

The release-profile test run moved out of the pull-request path: it now
lives in the release workflow, where the optimized build that is about to
ship is tested before it is packaged. The pull-request path does not pay
for a release-profile compile of everything.

The end-to-end worker count follows the runner: half the cores (a 4-core CI
runner gets 2 workers, an 8-core dev box 4), overridable with
`PLAYWRIGHT_WORKERS`. The readiness poll dropped from 200 ms to 25 ms —
the backend answers in ~200 ms, so the old interval added its full period
to every scenario's startup, and there is one startup per scenario.

## Measured in CI (PR 130, 2026-10-22, warm caches)

| job                                 | before (one job) | after                                                       |
| ----------------------------------- | ---------------- | ----------------------------------------------------------- |
| `checks-n-tests`                    | 10m08s           | split into two parallel jobs                                |
| ├─ `backend`                        | —                | 4m03s                                                       |
| │ setup                             | —                | 1m04s (the rust cache restore dominates)                    |
| │ the frontend build                | —                | 12s                                                         |
| │ clippy + utils + the API contract | —                | 1m10s                                                       |
| │ the backend + utils suites        | —                | 1m34s                                                       |
| └─ `frontend`                       | —                | 5m57s                                                       |
| setup                               | —                | 1m01s                                                       |
| the frontend + docs gates           | —                | 34s                                                         |
| vitest + the end-to-end suite       | —                | 4m19s (of which ~2m is the backend compile the suite needs) |
| `audit`                             | 2m06s            | **44s**                                                     |
| `docs`                              | (inside the job) | 21s                                                         |
| CodeQL (rust)                       | 8m39s            | 9m03s (unchanged)                                           |

The pull-request gate is now the slower of the two jobs instead of their
sum. The frontend job's gates still report at ~1.5 min, because they do not
wait for the backend job.

Two runs of the same PR, both with warm caches, differ by minutes: the
`backend` job took 4m03s and then 8m34s, the `frontend` job 5m57s and then
4m22s. The rust cache is the swing factor — a job that misses it pays the
cold clippy and build — and the shared runners add their own noise. The
honest reading: the gate is 4-6 min when the caches hit and up to ~8.5 min
when they do not, against ~10 min for the single job it replaces.

Two things the split surfaced, both fixed here: the frontend job needs
ExifTool (the scenarios generate their fixture images through snapfab,
which spawns it) and the end-to-end suite needs a backend binary — it
builds one itself, in parallel with the backend job, because waiting for
the backend job's artifact would have put the suite behind the whole
backend run.

## The main-branch pipeline (before this branch)

The release-profile job ran on every push to `main` (it was skipped only for
pull requests), so the main pipeline carried the LTO release build and the
release test suite on every merge:

| run         | `checks-n-tests` | `test-release-build` | pipeline |
| ----------- | ---------------- | -------------------- | -------- |
| 39719011830 | 9m22s            | 10m45s               | 20m07s   |
| 39445584218 | 10m50s           | 9m07s                | 19m57s   |
| 39423820490 | 8m49s            | 9m34s                | 18m23s   |

The release-profile job now lives in the release workflow, where the
optimized build that is about to ship is tested before it is packaged. The
main pipeline is the split jobs only: ~6 min instead of ~20 min.

## The binaries the CI installs

The CI used to install unpinned binaries: `cargo install cargo-deny` (a
source build), the plan tool (a source build from git), the runner's apt
ffmpeg, and — in the release job — the static ffmpeg from a _floating_ URL
at packaging time. That last one put an untested binary in the admin's
hands: CI tested the runner's ffmpeg, the package shipped whatever the URL
served that day.

Every tool is now installed from a pinned release with a checked digest:

| tool       | source                                       | pinned by        |
| ---------- | -------------------------------------------- | ---------------- |
| nextest    | the prebuilt release                         | version + sha256 |
| cargo-deny | the prebuilt release                         | version + sha256 |
| exiftool   | the pinned distribution                      | version + sha256 |
| ffmpeg     | the pinned static build (the one that ships) | version + sha256 |

The ffmpeg install is the one with teeth: the release package now ships
the very copy the release-mode tests ran against, and the floating URL is
gone.

## Decisions

- 2026-10-22: adopted candidates 1-5 (nextest, the double `npm ci`, the
  concurrent type checks, the prebuilt cargo-deny, the plan tool out of
  CI) and the job split. Candidates 6-7 stay open (spectral pinning, the
  release-mode job's own split); candidate 8 is partly addressed (the
  worker count follows nproc, the poll interval shrank); candidate 9
  (Biome) is assessed above and not adopted for CI time.
