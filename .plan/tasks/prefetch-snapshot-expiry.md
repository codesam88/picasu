---
status: open
type: bug
priority: high
area: backend
---

**`prefetch` snapshots deleted almost immediately instead of living for 1 hour.**

`Expire::expired_check` cannot distinguish "timestamp was never recorded" from "this is the current `VERSION_COUNT_TIMESTAMP`, whose expiry hasn't been scheduled yet." Both collapse to `None`. Under concurrent write activity, a freshly-prefetch'd snapshot can vanish within milliseconds.

Fix: distinguish "unrecorded" from "active, not yet scheduled" — e.g. store expiry as a tri-state, or only insert the current version's row once its successor exists.

Worked around in tests via `read_current_abstract_data`; not yet worked around in the frontend.
