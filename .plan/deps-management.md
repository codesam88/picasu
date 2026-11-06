---
status: in-progress
type: chore
priority: high
area: tooling
---

## Notes

Steps 1–8 are implemented (2026-11-06, branch `chore/deps-cleanup`).
Remaining: renovate rule, `just` version plumbing in CI, shared fetch
helper (backlog). Per-step status is recorded under Steps.

Landed shape (differs from the step-1 sketch): the manifest is
`scripts/deps/versions.env`, not `tools-pin.env` at root; the installer
is a run-parts dispatcher `scripts/deps-install.sh` over 14
`scripts/deps/*.sh` shims, not a single installer. `just` recipes
delegate to it; no version vars remain in the justfile. Deviations: no
shared verify helper (each shim duplicates fetch+sha logic — backlog).
`.github/renovate.json` covers the GitHub/PyPI/crate pins (ffmpeg +
exiftool stay manual; sha sidecars refresh by hand on bump PRs).

Re-verified 2026-11-06 (original survey 2026-11-04):

- `taiki-e/install-action` is SHA-pinned at 10 sites, but every site
  uses bare `tool: just` — `just` itself still floats in CI (open).
- Node `node-version: "24"` literals remain at 6 sites; accepted
  exception (`setup-node` needs a literal, manifest range check-only).
- Release no longer floats stable: plain `rustup target add`, so
  `rust-toolchain.toml` (1.99.1) governs, same as CI.
- No `curl * install.sh | sh` remains; 4 uv blocks use
  `just install uv`. No inline `UV_VERSION` in workflows.
- dylint justfile var gone; manifest `DYLINT_VERSION` + `dylint.sh`
  (`cargo install --locked`); 4 crates pin 6.3.1 via crates.io
  (cannot be single-sited — accepted, no drift check).
- mdbook manifest entry wired into `docs-build`.
- oasdiff per-platform assets+shas; only x86_64 Linux exercised in CI
  (known limitation).
- schemathesis/zizmor stay uvx-pinned, registry-verified, no sha
  sidecar (accepted). Incidental fix: `zizmor.yml` SARIF job read a
  nonexistent just variable; now greps the manifest.
- Media-tools caches rekeyed to `scripts/deps/versions.env`; release
  cache step removed (`cache-mode: none`, poisoning risk).
  Playwright-in-release stays uncached (opt-out under the same mode).
- New arrivals carry no pins: `gates-check`
  (`scripts/quality-gates.mjs`, node stdlib only) and `utils/bench`
  (`ruff 0.17.3` in its own justfile, venv-installed, dev-only, not
  CI-gated — accepted exception alongside the uvx pins).

## Goal

Single `tools-pin.env` at repo root defines every system-tool version
plus integrity data; single idempotent `scripts/deps-install.sh`
consumes it for local setup (`~/.local/bin`, no sudo); workflows keep
fast cached installs but read the same file, never re-declare versions.
Project deps (`Cargo.lock`, `frontend/package-lock.json`) stay out —
installed via `cargo`/`npm`, not this script. Language toolchains
(rust, node) stay out of install scope — script only checks and reports
supported versions, never imposes a compiler/interpreter.

## Strategy options

Evaluated against: single source machine-readable, bot-bumpable
(renovate/dependabot), sha verification per pin, local+CI parity via
a `just` wrapper, low churn for ~12 slow-moving pins.

(a) `.tool-versions` / mise (`mise.toml` + lock). Off-the-shelf
version manager; `mise.toml` is machine-readable TOML; renovate has a
mise manager; `mise install` + `mise exec` gives local+CI parity once
mise itself is bootstrapped. Downside: coverage — language runtimes
(node, uv, just) fit, but sha-pinned native tarballs
(ffmpeg/exiftool/oasdiff/nextest/cargo-deny/mdbook) map poorly to
mise backends, and per-pin sha256 is not the native verification
model. Adds a bootstrap dependency (mise) on every dev machine and
CI job; Rust already has `rust-toolchain.toml` outside mise.

(b) `tools/` manifest (`tools-pin.env` + single installer reading it,
with a `sha256` field per pin). One purpose-built source of truth,
e.g. `tools-pin.env` with `{ version, sha256, asset_template }` per
tool; a single `scripts/install-tool.sh <name>` (or one
`deps-install.sh` loop) does fetch+sha-verify+unpack; `just`
recipes become thin wrappers (`just install-dev` ->
`deps-install.sh --all`). Bot-bumpable via renovate custom-regex
manager on `tools-pin.env`; local+CI parity by construction (same
script, same manifest). Downside: custom loader to own (~100 lines
of shell + manifest schema), instead of an upstream tool.

(c) Nix / devcontainer. Reproducible dev+CI closures. Downside:
heaviest lift (flake, binary caches, macOS support, contributor
retraining) and orthogonal to the immediate gaps (unverified uv
pipe, oasdiff platform). Rejected for this plan; revisit only if
contributor-setup pain demands a dedicated environment plan.

(d) Keep justfile vars but generate them from the manifest.
Not a separate source — the justfile stays as a derived
convenience layer (`just --list` wrappers, version echo for cache
keys) while the manifest from (a) or (b) remains authoritative.
Viable as a complement to whichever of (a)/(b) wins; rejected as a
standalone solution because generated vars alone add a codegen step
without fixing verification or bot-bumping.

Recommendation: (b) manifest + single installer as `tools-pin.env` +
`scripts/deps-install.sh [--tool <name>] [--check]`, with (d) as thin
`just` wrapper (`just setup` -> script). Rationale: it is the only option
that covers all ~12 pins including odd native tarballs with per-pin sha256,
stays grep-able for workflows and bot-bumpable via single regex rule,
keeps local on one idempotent code path, adds no bootstrap. (a) mise is
fallback if upstream tooling preferred. Local installs to `~/.local/bin`,
no sudo; flag is `--tool`, not `--only`.

Rejected per user interview: justfile-vars-alone centralization
("silly" — justfile is prose/make-like, not machine-readable, not
bot-bumpable, and workflows still need plumbing to read it), and
distributing versions across per-tool `install-*.sh` scripts
(worse — N definition sites instead of one).

## Steps

1. Add the manifest + single installer. Status: done (actual shape in
   Notes; renovate config added at `.github/renovate.json`).
   Scope: new `tools-pin.env` (`TOOL_VERSION` + `TOOL_SHA256` + asset
   template per tool — nextest, cargo-deny, ffmpeg, exiftool, oasdiff,
   dylint, mdbook, uv, just), new `scripts/deps-install.sh`
   (`--tool <name>` to select, `--check` for drift check; fetch,
   sha-verify, unpack to `~/.local/bin`; per-platform assets so oasdiff
   macOS/aarch64 works), renovate custom-regex rule for the file.
   rust + node excluded from install: `tools-pin.env` holds
   `RUST_SUPPORTED` / `NODE_SUPPORTED` ranges, script only
   `rustc --version` / `node --version` check and report, never install.
   Done: `scripts/deps/versions.env` is the only version definition site
   (`grep -rn <version>` shows one hit); installer refuses on sha
   mismatch; `deps-install.sh --check` clean; `just plan-lint` clean.
   Note: uvx-pinned schemathesis/zizmor stay registry-verified
   (documented exception, no sha sidecar); Playwright stays on
   `package-lock.json`. Project deps excluded: `Cargo.lock` /
   `package-lock.json` handled by `cargo`/`npm`.

2. Thin `just` wrapper layer (option (d) as complement). Status: done
   (delegation landed; `UV_VERSION` gone from workflows; Node literals
   stay as accepted exception).
   Scope: `justfile` (install recipes delegate to
   `deps-install.sh`; no hand-maintained version vars, or vars
   echoed from the manifest for cache keys), `.github/workflows/*`
   (replace inline `UV_VERSION` with manifest-derived env; keep
   `setup-node` / rustup toolchain setup as version-check, not install
   from manifest).
   Done: `grep -rn UV_VERSION .github/` shows no inline definition;
   `just --list` exposes the install recipes; CI run green.

3. Verify the uv installer. Status: done (4 blocks use
   `just install uv`; no `install.sh | sh` remains).
   Scope: `deps-install.sh` uv entry (sha-pinned tarball) or pinned
   `astral-sh/setup-uv` action driven by the manifest value.
   Done: no `curl * install.sh | sh` remains (`grep -rn
"install.sh | sh"` empty); install path is sha/pin-verified;
   CI run green.

4. Check toolchains, do not install. Status: done (release uses
   `rust-toolchain.toml` via plain rustup; no `stable` float; `--check`
   scripts report ranges, no CI gate — accepted).
   Scope: `tools-pin.env` (`RUST_SUPPORTED`, `NODE_SUPPORTED` ranges),
   `scripts/deps-install.sh` check functions, `docs/` supported-versions
   note, `.github/workflows/release.yml` + `ci.yml` (keep
   `rust-toolchain.toml` / `setup-node` as now, add `--check` gate).
   `rust-toolchain.toml` stays source of truth for CI pin; out of
   `tools-pin.env` install scope.
   Done: `deps-install.sh --check` reports rust/node mismatch with
   supported range; no `stable` float in release toolchain step.

5. Pin mdbook or remove it. Status: done (kept; manifest entry wired
   into `docs-build`).
   Scope: `tools-pin.env` (new `mdbook` entry if kept), `justfile`
   (`docs-build`), `docs/book.toml` if kept.
   Done: either manifest `mdbook` entry + sha install wired into
   `docs-build`, or recipe removed with docs updated;
   `grep -rn mdbook` shows no unpinned invocation;
   `just docs-build` passes.

6. Fix oasdiff portability or document linux-only. Status: done
   (per-platform assets; non-x86_64 paths untested — known limitation).
   Scope: `tools-pin.env` (per-platform assets+shas),
   `deps-install.sh`, `scripts/openapi-breakcheck.sh`, `justfile`
   (`openapi-breakcheck`), CI job definition.
   Done: either per-platform asset+sha (macOS/aarch64) installed, or
   an explicit documented linux-only gate with a skip message
   elsewhere; `just openapi-breakcheck` passes on CI platforms.

7. Dedup dylint into the manifest. Status: done (crates aligned at
   6.3.1, lint README fixed; no drift check — accepted).
   Scope: `tools-pin.env` (`dylint` entry),
   `utils/dylint-lints/*/Cargo.toml` (4 crates), installer.
   Done: crate pins track `DYLINT_VERSION` (single-siting impossible
   for crates.io deps — dependabot-cargo-owned);
   `just install dylint` and `just dylint-check` pass.

8. Tighten playwright/media cache keys. Status: done (keys hash the
   manifest; release cache removed; Playwright-in-release uncached by
   design).
   Scope: `.github/workflows/{ci,release}.yml` (cache steps), frontend
   browser install step.
   Done: media-tools keys hash `scripts/deps/versions.env`; release
   has no media cache (`cache-mode: none`) and no Playwright cache
   (opt-out under the same mode); CI run green.

## Out of scope

- Docker image provenance (separate plan).
- Release tarball signing (separate plan).
- `cargo deny`/`npm audit` policy changes; only their installer pins move.
- Backend test parallelism: per AGENTS.md, never run backend test
  commands concurrently with other work touching the same tree.
- Bench harness pins (`ruff_version` in `utils/bench/justfile`,
  venv-installed, dev-only) and benchmark input manifests.
- npm-layer pins (`package.json`/`package-lock.json`, one-shot
  `widdershins`, spectral CLI) and the `tablethat` git-rev pin.
- `quality-gates.mjs` (node stdlib only, pattern-based, no pins).

## Verification

- `just plan-lint` clean (frontmatter `open`/`chore`/`high` per schema).
- Per step: grep transcript showing single definition site, the `just`
  recipe run, and the affected CI workflow run.
- Gates before handoff: `just check`, `just test` per AGENTS.md
  (`PICASU_WEAK_PWHASH=1` via the justfile; no concurrent backend tests).

## Interview

Settled 2026-11-04: deps-first direction confirmed; justfile-vars-alone
centralization rejected as silly (not machine-readable, not
bot-bumpable); distributing versions across per-tool install scripts
rejected as worse (N definition sites). New direction is manifest +
single installer per Strategy above (recommendation (b), fallback
(a) mise).

Settled during implementation (2026-11-06): manifest format is
`scripts/deps/versions.env` shell schema; dispatcher is a
`deps-install.sh` loop over per-tool shims; Node stays major `24`
literal in workflows with a check-only range in the manifest; uv goes
through the sha-pinned tarball via `just install uv`; mdbook is kept
(manifest entry + `docs-build` wiring); CI installs
`just@${{ env.JUST_VERSION }}` with the version read from the manifest
into `$GITHUB_ENV` (action stays SHA-pinned, upstream sha-verified
install, same cached path).

No open decisions remain; leftover work is tracked as backlog (shared
fetch helper, non-x86_64 installer verification).
