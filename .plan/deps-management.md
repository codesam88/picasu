---
status: in-progress
type: chore
priority: high
area: tooling
---

## Notes

External binary/tool versions are scattered across four layers with no
single source of truth: `justfile` vars, `scripts/install-*.sh` args,
per-workflow `env` (e.g. `UV_VERSION`), and floating installer refs
(`just`, `node`, `rust-stable`). Install verification is uneven: some
tools pin version+sha256, others pipe `curl|sh` unverified.

Survey evidence (2026-11-04, `main`):

- `just`: `taiki-e/install-action` without version floats (`ci.yml:46`);
  same unpinned ref duplicated across ci/release/msrv/fuzz/zizmor/docs/audit.
- Node: major-only `"24"` duplicated in ci/release/fuzz/audit/docs-check.
- Rust: `release.yml` uses `dtolnay/rust-toolchain@... # stable` while
  `rust-toolchain.toml` pins `1.99.1` — release does not build on the
  repo toolchain.
- uv: `UV_VERSION: "0.12.18"` in 3 workflows (ci, fuzz-advisory, zizmor x2
  jobs) with `curl -LsSf https://astral.sh/uv/install.sh | sh`, no sha.
- Pinned+sha in justfile+scripts: nextest `0.9.138`, cargo-deny `0.20.2`,
  ffmpeg `7.0.2`, exiftool `13.59`, oasdiff `1.33.0` — the good pattern.
- dylint `6.3.1`: duplicated in justfile `dylint_version` plus 4 lint
  crates (`utils/dylint-lints/*/Cargo.toml` x `dylint_linting`/`dylint_testing`).
- mdbook: invoked unpinned in `docs-build`, no version anywhere.
- oasdiff: `install-oasdiff.sh` fetches `linux_amd64` only; macOS/dev and
  aarch64 CI cannot install it.
- schemathesis `4.30.1` / zizmor `1.31.1` via `uvx <tool>@<version>`:
  version-pinned, integrity via uv registry, no sha sidecar (accepted).
- Playwright: implicit via `package-lock.json`; browser install
  (`npx playwright install chromium`) uncached in release.yml.
- Media-tools cache (`actions/cache`, keyed on justfile) exists in
  ci.yml/release.yml but keys do not cover all pinned inputs.

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

1. Add the manifest + single installer.
   Scope: new `tools-pin.env` (`TOOL_VERSION` + `TOOL_SHA256` + asset
   template per tool — nextest, cargo-deny, ffmpeg, exiftool, oasdiff,
   dylint, mdbook, uv, just), new `scripts/deps-install.sh`
   (`--tool <name>` to select, `--check` for drift check; fetch,
   sha-verify, unpack to `~/.local/bin`; per-platform assets so oasdiff
   macOS/aarch64 works), renovate custom-regex rule for the file.
   rust + node excluded from install: `tools-pin.env` holds
   `RUST_SUPPORTED` / `NODE_SUPPORTED` ranges, script only
   `rustc --version` / `node --version` check and report, never install.
   Done: `tools-pin.env` is the only version definition site
   (`grep -rn <version>` shows one hit); installer refuses on sha
   mismatch; `deps-install.sh --check` clean; `just plan-lint` clean.
   Note: uvx-pinned schemathesis/zizmor stay registry-verified
   (documented exception, no sha sidecar); Playwright stays on
   `package-lock.json`. Project deps excluded: `Cargo.lock` /
   `package-lock.json` handled by `cargo`/`npm`.

2. Thin `just` wrapper layer (option (d) as complement).
   Scope: `justfile` (install recipes delegate to
   `deps-install.sh`; no hand-maintained version vars, or vars
   echoed from the manifest for cache keys), `.github/workflows/*`
   (replace inline `UV_VERSION` with manifest-derived env; keep
   `setup-node` / rustup toolchain setup as version-check, not install
   from manifest).
   Done: `grep -rn UV_VERSION .github/` shows no inline definition;
   `just --list` exposes the install recipes; CI run green.

3. Verify the uv installer.
   Scope: `deps-install.sh` uv entry (sha-pinned tarball) or pinned
   `astral-sh/setup-uv` action driven by the manifest value.
   Done: no `curl * install.sh | sh` remains (`grep -rn
"install.sh | sh"` empty); install path is sha/pin-verified;
   CI run green.

4. Check toolchains, do not install.
   Scope: `tools-pin.env` (`RUST_SUPPORTED`, `NODE_SUPPORTED` ranges),
   `scripts/deps-install.sh` check functions, `docs/` supported-versions
   note, `.github/workflows/release.yml` + `ci.yml` (keep
   `rust-toolchain.toml` / `setup-node` as now, add `--check` gate).
   `rust-toolchain.toml` stays source of truth for CI pin; out of
   `tools-pin.env` install scope.
   Done: `deps-install.sh --check` reports rust/node mismatch with
   supported range; no `stable` float in release toolchain step.

5. Pin mdbook or remove it.
   Scope: `tools-pin.env` (new `mdbook` entry if kept), `justfile`
   (`docs-build`), `docs/book.toml` if kept.
   Done: either manifest `mdbook` entry + sha install wired into
   `docs-build`, or recipe removed with docs updated;
   `grep -rn mdbook` shows no unpinned invocation;
   `just docs-build` passes.

6. Fix oasdiff portability or document linux-only.
   Scope: `tools-pin.env` (per-platform assets+shas),
   `deps-install.sh`, `scripts/openapi-breakcheck.sh`, `justfile`
   (`openapi-breakcheck`), CI job definition.
   Done: either per-platform asset+sha (macOS/aarch64) installed, or
   an explicit documented linux-only gate with a skip message
   elsewhere; `just openapi-breakcheck` passes on CI platforms.

7. Dedup dylint into the manifest.
   Scope: `tools-pin.env` (`dylint` entry),
   `utils/dylint-lints/*/Cargo.toml` (4 crates), installer.
   Done: version defined once in the manifest
   (`grep -rn "6\.3\.1"` shows manifest + generated lock/derived
   uses only); `just install dylint` and `just dylint-check` pass.

8. Tighten playwright/media cache keys.
   Scope: `.github/workflows/{ci,release}.yml` (cache steps), frontend
   browser install step.
   Done: cache keys cover manifest hash + lockfile + browser version;
   `npx playwright install chromium` cache-hit on re-run; CI run green.

## Out of scope

- Docker image provenance (separate plan).
- Release tarball signing (separate plan).
- `cargo deny`/`npm audit` policy changes; only their installer pins move.
- Backend test parallelism: per AGENTS.md, never run backend test
  commands concurrently with other work touching the same tree.

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

Open decisions for the user:

- Manifest format: `tools-pin.env` custom schema + renovate custom-regex
  vs `mise.toml` upstream (accepts weaker sha coverage for native
  tarballs)?
- Single `deps-install.sh` loop vs per-tool shim calling a shared
  verify helper?
- Node: exact version in manifest (e.g. `24.x.y` + sha via setup-node)
  vs major `24` (less churn, less reproducibility)?
- uv verification method: sha-pinned tarball in manifest,
  `astral-sh/setup-uv` action pinned to the manifest value, or
  cosign-checked tarball?
- mdbook: keep (add manifest entry) or drop (`docs-build` removed,
  book served another way)?
- just provisioning in CI: pinned `taiki-e/install-action` driven by
  the manifest vs `cargo-binstall`-installed just?
