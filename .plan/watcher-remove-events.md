---
status: in-progress
type: feature
priority: high
area: backend
---

Tracked in `pre01.md` (meta) as 1b.

## Context

`start_watcher.rs` handles `Create` and `Modify` events; `Remove` events fall
through to `_ => {}`. A file deleted externally stays in the DB as a stale
record with a broken alias. Breaks the "filesystem as source of truth" promise.

## Tasks

- [ ] Handle `EventKind::Remove(_)` in the watcher: look up the path in
      `DATA_TABLE` (scan aliases), remove the alias from the record, and if
      no aliases remain, remove the whole record + thumbnail.
- [ ] On manual album indexing (`POST /post/index/album`), the sweep after
      scanning new files should also check existing DB records under the target
      path for dead aliases.
