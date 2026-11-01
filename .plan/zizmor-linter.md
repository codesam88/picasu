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
under `[group('deps')]`. Remaining follow-up (separate slice): baseline
triage of findings (`zizmor-check` currently exits 14), CI job.
