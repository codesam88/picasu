---
status: open
type: chore
priority: high
area: devops
---

## Problem

`release.yml` (196 lines) publishes install tarballs with no integrity,
changelog, or version truth. Zero tags exist, so the workflow has never
been exercised.

## Survey evidence

Flow: tag `[0-9]*` / `test` branch / dispatch triggers; `codeql` gate
with a malformed reusable-workflow ref (`uses: $/.github/workflows/codeql.yml:30`);
`build-linux` on ubuntu-22.04 x86_64 only; frontend build + `cargo build`
with `embed-frontend`; `--check-openapi` subset gate (vs full
`just openapi-check`); media tools cached + digest-checked; tarball
version from `GITHUB_REF_NAME` with `0.0.0` fallback; `gh release create
--notes ""` with no sha256 and no SBOM.

Gaps:

- G1 — no sha256 checksums published alongside the tarballs.
- G2 — no SBOM and no attestation (`id-token` permission missing).
- G3 — release notes are empty (`--notes ""`).
- G4 — no version cross-check: Cargo `0.1.0` vs frontend `2.1.0` vs tag
  are never compared.
- G5 — no `concurrency` block; overlapping release runs can interleave.
- G6 — `cache-mode: none` is ineffective (not a real workflow key) yet
  `npm/cache` and `actions/cache` are still used.
- G7 — `contents: write` with no `environment` protection on the
  publish step.
- G8 — malformed reusable-workflow ref (line 30); the CodeQL gate cannot
  resolve as written.
- G9 — single-arch build (x86_64 Linux only).
- G10 — subset OpenAPI gate (`--check-openapi` only) instead of the full
  `just openapi-check`.

## Strategy options

(a) Minimal hardening in the current workflow: fix the ref, add
concurrency, publish sha256, fill in notes by hand per release.

(b) Tag-check + changelog tooling (git-cliff or generated notes) +
sha256 + concurrency + reusable-ref fix.

(c) Full supply-chain posture: SLSA attestation + SBOM + multi-arch
builds + environment protection on the publish job.

Recommendation (phased, settled 2026-11-04): phase 0 — correctness
(fix reusable ref, concurrency, `cache-mode` comment or removal,
version cross-check where the tag drives and Cargo/frontend mismatch
fails); phase 1 — integrity (sha256 checksums now + git-cliff
CHANGELOG with committed config driving release notes); phase 2 —
supply-chain deferred (SBOM + attestation wait until phases 0–1 have
shipped at least one tagged release). See Interview for what is
settled vs still open.

Rejected / drawbacks: jumping straight to (c) adds provenance
machinery to a workflow that has never run once — untestable weight.
Staying at (a) leaves version truth and changelog generation manual,
which drifts under release pressure. Multi-arch in phase 0 doubles
build/test cost before single-arch release is proven.

## Steps

1. Phase 0 correctness, `.github/workflows/release.yml` only: fix the
   `uses:` ref to `./.github/workflows/codeql.yml` (or the intended
   `org/repo/path@ref` form); add a top-level `concurrency` block
   cancelling superseded release runs; resolve the `cache-mode: none`
   comment (remove the key or replace with an accurate comment about
   what is and is not cached); add a version cross-check step — the
   tag drives the version, fail on any Cargo (`Cargo.toml`) vs
   frontend (`package.json`) vs tag mismatch (no `0.0.0` silent
   fallback).
2. Phase 1 integrity, `release.yml` + `docs/linux.md` + `CHANGELOG.md`
   (`cliff.toml` committed): generate `sha256sums.txt` over both
   tarballs, upload it as an artifact and attach it to the release;
   generate release notes from git-cliff (committed `cliff.toml`
   config; changelog excerpt, never `--notes ""`); document the
   checksum verification step in `docs/linux.md`.
3. Phase 2 supply-chain (deferred), `release.yml`: add `id-token:
write`, emit an SBOM and build attestation. Explicitly out of
   phases 0–1; starts only after one tagged release has shipped on
   the phase 0+1 workflow.
4. Gate review: `zizmor` clean on the workflow, YAML lint
   (`actionlint` if available, else `yamllint`), and `grep`
   transcripts showing the old failure modes gone (malformed `uses:`,
   `--notes ""`, `0.0.0` fallback without check).

Out of scope: Docker image provenance and dependency centralization
(separate plan).

## Done criteria

- `zizmor` reports no findings on `release.yml`.
- Workflow YAML lint passes (`actionlint` if available, else
  `yamllint`).
- `grep` transcripts attached to the change: no `uses: $/`, no
  `--notes ""`, version check present, sha256 step present.
- No tag was pushed to verify (dry-run / lint only).

## Verification

- `just plan-lint` (frontmatter gate for this file).
- `actionlint` if available, otherwise `yamllint`, on the edited
  workflow — run at implementation time, not here.
- Explicitly out: pushing a tag to exercise the workflow for real.

## Interview

Settled 2026-11-04:

- Version source of truth: tag drives everything; cross-check fails
  on Cargo/frontend mismatch.
- Changelog tool: git-cliff with committed config (`cliff.toml` +
  `CHANGELOG.md`); release notes come from it, never `--notes ""`.
- SBOM/attestation: deferred to phase 2; phase 1 ships checksums now.

Still open:

1. Stay single-arch (x86_64) for the first hardened release, or add
   aarch64 from the start?
2. Environment protection on the publish step: required reviewers /
   deploy environment, or keep `contents: write` unguarded?
