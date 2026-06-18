---
status: open
type: bug
priority: high
area: backend
---

Three watcher correctness bugs pinned by red tests (tests fail intentionally to flag the gaps):

1. **scenario_l** — Watcher indexing of a file inside a dir-album directory does NOT set the explicit `album` field to match path membership. `ensure_dir_albums` creates album records per directory level but never sets the photo's own `album` field.

2. **scenario_m** — Same hash rediscovered at its current alias path (e.g. after `assign_album`) creates a duplicate alias entry instead of recognizing it as unchanged.

3. **scenario_r** — Same hash rediscovered at a new path (file moved externally) does not prune dead alias entries from the old record.

See `docs/test-strategy.md` regression matrix rows 1–3.
