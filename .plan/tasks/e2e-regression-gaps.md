---
status: open
type: feature
priority: medium
area: testing
---

E2E regression gap scenarios — all require no generator/DSL changes, just new YAML files:

1. Share capability flags (`show_metadata`, `show_download`, `show_upload`) — create share with flags, verify metadata stripped / download suppressed / upload gated
2. `assign_album` targeting a manual album (no `dir_path`) → 4xx — confirm error on non-dir albums
3. Stale `DIR_ALBUM_CACHE` — dir deleted externally → clear error (needs `delete_dir` DSL verb first)
4. `POST /upload` with hash already known but different album → last-write-wins
5. `GET /object/*` with wrong hash-scoped token → 401/403
6. Prefetch snapshot expiry — blocked until prefetch-expiry bug is fixed

See `docs/test-strategy.md` regression matrix for expected behaviors.
