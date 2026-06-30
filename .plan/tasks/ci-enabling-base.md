---
status: done
type: chore
priority: high
area: devops
---

- **Enable CI** to delegate regression tests and enforce project QA
  - PR/merge CI: developer config (debug, no embed-frontend) — matches local precommit
  - release CI: production config (release + embed-frontend) — already in `.github/workflows/release.yml`, but not yet gated as a required PR check before release
  - regular dependency audits - establish a regular cadence (npm-check-updates, Dependabot, or Renovate); small frequent updates cheaper than batched drift
