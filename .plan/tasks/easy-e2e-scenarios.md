---
status: open
type: feature
priority: medium
area: testing
---

Easy E2E scenario coverage — 12 scenarios, single YAML file each, zero generator changes needed:

1. `authenticate_succeeds_with_no_password` — `POST /post/authenticate` → 200 + token
2. `create_share_then_read` — `POST /post/create_share` on dir-album, verify `share_list` via `/get/albums`
3. `set_album_cover_updates_album` — `PUT /put/set_album_cover`, verify via `/get/albums`
4. `set_album_title_updates_album` — `PUT /put/set_album_title` with display title
5. `rotate_image_swaps_dimensions` — `PUT /put/rotate-image`, verify width/height swap via `/get/get-data`
6. `config_read_endpoints` — `GET /get/config` and `GET /get/config/export` smoke test
7. `edit_flags_then_verify` — prefetch → capture index → `PUT /put/edit_flags` → verify flag change
8. `delete_data` — prefetch → `DELETE /delete/delete-data` → verify file absent
9. `index_image_single` — `POST /post/index/image` → 202
10. `cancel_album_index` — `POST /post/index/cancel` → 200
11. `get_index_status` — `GET /get/index/status` → 200 + JSON status
12. `get_rows_get_scroll_bar` — prefetch → `GET /get/get-rows`/`get-scroll-bar` with Bearer token
