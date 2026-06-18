---
status: open
type: chore
priority: medium
area: devops
---

Expand CI to test all supported configurations on PR and release:

- PR/merge CI: developer config (debug, no embed-frontend) — matches local precommit
- release CI: production config (release + embed-frontend) — already in `.github/workflows/release.yml`, but not yet gated as a required PR check before release
