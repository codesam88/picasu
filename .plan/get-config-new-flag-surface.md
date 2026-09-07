---
status: open
type: feature
priority: low
area: backend
---

`GET /get/config` surfaces only one of the three upload-behavior flags added with the upload hardening work. Raised in
PR \#17 review (2026-09-07); confirm intent and decide.

## Context

`ConfigResponse` (`backend/src/router/get/get_config.rs:24-27`) exposes `validate_upload_content`, but not
`normalize_upload_filenames` or `use_client_timestamp_info`, even though all three exist in `AppConfig`, are persisted
to TOML, and are editable via `PUT /put/config` (`edit_config.rs:91-99`). `GET /get/config` needs an auth context
(`GuardShare`) but is the only read-side view of effective config.

Possible positions:

- By design: the response only carries flags the **frontend UI actively branches on** (`read_only_mode`, `disable_img`,
  `fs_notify_watcher`, `validate_upload_content`). The upload-options dialog renders static rule copy and does not read
  these flags.
- Gap: an operator cannot observe or remotely diagnose effective `normalize_upload_filenames` /
  `use_client_timestamp_info` state except via the TOML file, and `PUT /put/config` accepts updates to fields the GET
  response doesn't report back — a mild asymmetry.

## Tasks / decision

- [ ] Confirm whether the subset is intentional and document it on `ConfigResponse` (if so, close this ticket as
      `decision`).
- [ ] Otherwise add the two flags to `ConfigResponse` and cover with a backend scenario asserting the round-trip through
      `PUT /put/config`.
