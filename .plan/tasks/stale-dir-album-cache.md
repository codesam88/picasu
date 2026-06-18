---
status: open
type: bug
priority: high
area: backend
---

**Stale `DIR_ALBUM_CACHE` scenario** — if a directory is deleted externally while the cache still holds its entry, `assign_album` will attempt to move a file into a non-existent path.

Decide: detect and evict stale entries at startup, or return a clear 400 with a meaningful message at request time.

Also tracked in the FS/DB consistency reporting survey (situation #3, #6).
