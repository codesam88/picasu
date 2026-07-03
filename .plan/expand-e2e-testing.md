---
status: open
type: feature
priority: medium
area: testing
---

Expand e2e testing for API and UI

- [ ] `authenticate_succeeds_with_no_password` — `POST /post/authenticate` → 200 + token
- [ ] `create_share_then_read` — `POST /post/create_share` on dir-album, verify `share_list` via `/get/albums`
- [ ] `set_album_cover_updates_album` — `PUT /put/set_album_cover`, verify via `/get/albums`
- [ ] `set_album_title_updates_album` — `PUT /put/set_album_title` with display title
- [ ] `rotate_image_swaps_dimensions` — `PUT /put/rotate-image`, verify width/height swap via `/get/get-data`
- [ ] `config_read_endpoints` — `GET /get/config` and `GET /get/config/export` smoke test
- [ ] `edit_flags_then_verify` — prefetch → capture index → `PUT /put/edit_flags` → verify flag change
- [ ] `delete_data` — prefetch → `DELETE /delete/delete-data` → verify file absent
- [ ] `index_image_single` — `POST /post/index/image` → 202
- [ ] `cancel_album_index` — `POST /post/index/cancel` → 200
- [ ] `get_index_status` — `GET /get/index/status` → 200 + JSON status
- [ ] `get_rows_get_scroll_bar` — prefetch → `GET /get/get-rows`/`get-scroll-bar` with Bearer token

Older items:

- [ ] Share capability flags (`show_metadata`, `show_download`, `show_upload`) — create share with flags, verify metadata stripped / download suppressed / upload gated
- [ ] `assign_album` targeting a manual album (no `dir_path`) → 4xx — confirm error on non-dir albums
- [ ] Stale `DIR_ALBUM_CACHE` — dir deleted externally → clear error (needs `delete_dir` DSL verb first)
- [ ] `POST /upload` with hash already known but different album → last-write-wins
- [ ] `GET /object/*` with wrong hash-scoped token → 401/403
- [ ] Prefetch snapshot expiry — blocked until prefetch-expiry bug is fixed
