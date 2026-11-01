---
status: in-progress
type: feature
priority: medium
area: tooling
---

## Notes

Add zizmor (GitHub Actions static analysis) to the repo's pinned-tool
workflow: `install-zizmor` + `zizmor-check` just recipes backed by
`scripts/install-zizmor.sh`, following the schemathesis uvx pattern
(pinned version, integrity via uv registry; no upstream sha256 sidecar).

Decisions (2026-11-01): install via `uvx zizmor@<version>` (pre-compiled,
fast; CI backend job already installs uv); run offline at repo root
(`--offline`, no GH_TOKEN, deterministic). Latest pin: 1.31.1.

## Goal

`just install-zizmor` prefetches the pinned release; `just zizmor-check`
audits the repo offline. Wired in: `install-zizmor` in `install-dev`,
`zizmor-check` in `utils-check` and `check`; install recipes regrouped
under `[group('deps')]`.

## Baseline triage (2026-11-01, done — `zizmor-check` exits 0)

- template-injection: `${{ github.* }}` in `run:` replaced with `GITHUB_*`
  env vars; tainted `${{ env.* }}` uses moved through step-level `env:`.
- artipacked: `persist-credentials: false` on all 11 checkout steps (no
  git-push/token use in any workflow, verified by grep).
- dependabot-cooldown: `cooldown: {default-days: 7}` on all 4 updaters.
- cache-poisoning: workflow-level `cache-mode: none` in release.yml
  (documented remediation; release builds re-download verified tools).
- self-repository: `./.github/workflows/codeql.yml` → `$/...` (supported
  per GitHub changelog, July 2026).
- superfluous-actions: softprops/action-gh-release replaced with a
  `gh release create/upload` script step (create-or-upload, secrets via
  step `env:`).

## Remaining follow-up (separate slice)

CI job for the zizmor gate.
