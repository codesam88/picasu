---
status: done
type: bug
priority: medium
area: backend
---

### Progress (2026-09-25)

Implemented descendant cover selection for parent-only albums. The fallback
uses the newest eligible descendant image and ignores generated
`.__picasu_ph__.jpg` placeholders. Added unit coverage and an API scenario
verifying the parent album exposes the descendant asset as its cover.

### Progress (2026-09-20)

Reported by user. Needs investigation of `self_update()` cover selection
logic to determine whether sub-album images should be considered.
