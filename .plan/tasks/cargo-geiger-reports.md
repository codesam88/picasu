---
status: idea
type: chore
priority: low
area: devops
---

Periodic `cargo geiger` reports to track the unsafe surface area of the dependency tree. Not suitable for precommit (slow, requires full build). Include in release/audit reports.

See `docs/test-strategy.md` § "Tooling gaps."
