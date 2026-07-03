---
status: done
type: bug
priority: high
area: backend
---

**`prefetch` snapshots deleted almost immediately instead of living for 1 hour.**

`Expire::expired_check` cannot distinguish "timestamp was never recorded" from "this is the current `VERSION_COUNT_TIMESTAMP`, whose expiry hasn't been scheduled yet." Both collapse to `None`. Under concurrent write activity, a freshly-prefetch'd snapshot can vanish within milliseconds.

Fix: distinguish "unrecorded" from "active, not yet scheduled" — e.g. store expiry as a tri-state, or only insert the current version's row once its successor exists.

Worked around in tests via `read_current_abstract_data`; not yet worked around in the frontend.

## Progress

- Fixed: `Expire::expired_check` (`backend/src/storage/cache.rs`) treated a missing
  expire-table row as "already expired" (`None => true`). The only realistic source of
  a missing row is the race between `update_expire_task`'s atomic `VERSION_COUNT_TIMESTAMP`
  swap and its write-transaction commit — a row for an already-removed table can never be
  observed here, since the corresponding query-snapshot table would already be deleted and
  no longer appear in `expire_check_task`'s `list_tables()` scan. Changed the missing-row
  case to `false` (not expired) so a freshly rotated version isn't deleted before its expiry
  is actually scheduled. Added a regression test
  (`storage::cache::expire_tests::expired_check_does_not_expire_unscheduled_active_version`).
  No frontend workaround needed after this fix.
