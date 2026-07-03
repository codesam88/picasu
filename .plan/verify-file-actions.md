---
status: done
type: chore
priority: high
area: testing
---

Verify backend handling of basic file options via API and via filesystem

- Filesystem image created / removed / moved
  - paths: watcher, manual indexing
  - conflicts: api image move into manually created file (not indexed)
  - use cases to verify (check design.md)
    - new file, index and done
    - new file with same hash, add to alias[]
    - removed file, need to look for aliases to handle potential alias[0] change

- API image upload, move, delete
  - upload to target folder (rename/merge/skip)
  - move to target folder (rename/merge/skip)
  - delete (mark trash, confirm to delete)

2026-06-30: Done (pre01 branch). File lifecycle now fully handled:

- Watcher Remove events: alias pruned, record+thumbnail removed when last alias gone
- delete_data: removes original + sidecar + thumbnail from disk
- assign_album + upload: on_conflict=skip|rename|replace (rename uses photo-001.jpg pattern)
- E2E scenarios z1–z8:
  - z1: sidecar written on tag edit
  - z2: sidecar moves with file on assign_album
  - z3: delete removes file + sidecar from disk
  - z4: assign_album on_conflict=skip (no overwrite)
  - z5: assign_album on_conflict=rename → photo-001.jpg
  - z6: assign_album on_conflict=replace → overwrites destination
  - z7: on_conflict=rename with photo-001.jpg taken → photo-002.jpg
  - z8: batch assign (3 files sequentially to same album)
- Known gaps (out of scope for current runner): upload multipart conflict scenarios,
  watcher Remove integration, duplicate-hash alias-add scenario
