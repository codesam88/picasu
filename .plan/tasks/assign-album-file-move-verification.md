---
status: open
type: chore
priority: medium
area: testing
---

**`assign_album` file-move verification in tests** — Scenario H checks album membership after assignment but does not verify the file moved to the correct directory on disk. Add a filesystem assertion to close this gap.
