# Test Strategy

## Philosophy

Test logic the compiler cannot verify. Don't test what the type system, the framework,
or the serialization library already guarantee.

In practice this means:
- **Test pure functions** with non-trivial branches or invariants (filters, transforms, priority logic).
- **Test schema contracts** at boundaries where silent corruption is possible (bitcode positional encoding).
- **Test integration paths** where multiple components interact in ways the types don't constrain (index → dedup → flush → album update).
- **Don't test** Rocket routing, redb reads/writes, bitcode round-trips on unchanged structs, or Vue rendering internals.

---

## Current state

| Layer | Tool | Status |
|---|---|---|
| Backend format | `cargo fmt --check` | ✅ in precommit |
| Backend lint | `cargo clippy -- -D warnings` | ✅ in precommit |
| Unsafe code | `#![deny(unsafe_code)]` in `main.rs` | ✅ enforced at compile time |
| Backend unit + scenario tests | `cargo nextest run` (~70 tests, `src/tests/e2e.rs` scenarios A–S + unit tests) | ✅ full suite required on `main`; informational elsewhere (see `justfile`) |
| Frontend format | `prettier --check` | ✅ in precommit |
| Frontend types | `vue-tsc --noEmit` | ✅ in precommit |
| Frontend lint | `eslint` (strictTypeChecked + vue strongly-recommended) | ✅ in precommit |
| Frontend tests | Vitest (lexer only) | 🟡 minimal |
| Security audit | `cargo deny check` (licenses + advisories) | ✅ in `just audit` |
| E2E (Playwright, browser-level) | — | ❌ not started |

`src/tests/e2e.rs` is a single binary that boots a real Rocket instance against a
tempdir-backed redb, so "E2E" here means full-stack-minus-browser: real HTTP requests,
real indexing pipeline, real files on disk. A handful of scenarios (`scenario_l`,
`scenario_m`, `scenario_n`, `scenario_r`, plus two unit tests under
`extract_keywords::tests`) are *intentionally* red — they pin down known bugs/gaps (see
`TODO.md`, "Known bugs") so the fix shows up as a green diff later. `just precommit`
only enforces this passing on `main`; non-main branches can carry red tests while a fix
is in progress (see `justfile`'s `precommit` recipe).

---

## Input paths and handlers

Every way data enters or is read out of the system, grouped by area. "Mutates" lists
what redb state changes; "Guards" lists the request guards that gate access (see
`router/fairing/`) — these are the axes that matter for the operation-modes and
regression-matrix sections below.

### Ingestion (file → `AbstractData` record)

| Path / trigger | Handler | Mutates | Guards |
|---|---|---|---|
| Filesystem watcher `Create`/`Modify` event under a configured `sync_paths` entry | `start_watcher.rs` → debounced → `workflow::index_for_watch(path, None)` | `DATA_TABLE` (new or merged record), dir-album `DATA_TABLE` entries via `ensure_dir_albums` | none (background task, not a route) |
| `POST /post/import/folder` | `import_folder.rs` → `tasks/actor/folder_import.rs` → walks dir, calls `index_for_watch` per file | same as above, batched | `GuardAuth` + `GuardReadOnlyMode` |
| `POST /post/import/folder/cancel` | `import_folder.rs::cancel_folder_import_handler` | cancels in-flight folder import | `GuardAuth` |
| `GET /get/import/folder/status` | `get_import.rs` | read-only (progress poll) | `GuardAuth` |
| `POST /upload?<presigned_album_id_opt>` | `post_upload.rs` → `index_for_watch(path, Some(album_id))` | `DATA_TABLE`; sets `album` field via `presigned_album_id` (the *one* path where indexing-time album assignment already exists) | `GuardUpload` (admin JWT **or** a share's `show_upload` token) + `GuardReadOnlyMode` |
| `PUT /put/rotate-image` | `rotate_image.rs` → re-decodes, re-hashes, regenerates thumbnail in place | `DATA_TABLE` (width/height swap, phash/thumbhash, thumbnail file) | `GuardAuth` + `GuardReadOnlyMode` |
| `PUT /put/regenerate-thumbnail-with-frame` | `regenerate_thumbnail.rs` | `DATA_TABLE` (thumbnail, phash/thumbhash) from a user-supplied replacement frame | `GuardAuth` + `GuardReadOnlyMode` |
| `POST /put/reindex` | `reindex.rs` → `regenerate_metadata_for_image`/`_video` (mutates the **existing** record in place) | `DATA_TABLE` (exif, dimensions, hashes, thumbnail; tags/album preserved — see `scenario_s`) | `GuardAuth` + `GuardReadOnlyMode` |

### Albums

| Path / trigger | Handler | Mutates | Guards |
|---|---|---|---|
| Indexing under a multi-level `sync_paths` subdirectory | `workflow::ensure_dir_albums` (called from `index_for_watch`) | creates `Album` `AbstractData` records per directory level (`get_or_create_dir_album`); **does not** set the photo's own `album` field (`scenario_l`, red) | n/a |
| `POST /post/create_dir_album` | `create_dir_album.rs` | creates a subdirectory on disk + an `Album` record under an existing dir-album | `GuardAuth` + `GuardReadOnlyMode` |
| `PUT /put/assign_album` | `assign_album.rs` | renames the file on disk into the target album's directory, updates `alias`, sets explicit `album` field, marks old+new album for stats refresh | `GuardAuth` + `GuardReadOnlyMode` |
| `PUT /put/set_album_cover` | `edit_album.rs` | `Album.metadata.cover` | `GuardAuth` + `GuardReadOnlyMode` |
| `PUT /put/set_album_title` | `edit_album.rs` | `Album.metadata.title` | `GuardShare` (admin **or** a valid share token — broader than `set_album_cover`'s `GuardAuth`) + `GuardReadOnlyMode` |
| `AlbumSelfUpdateTask` (triggered after assign/delete/flag changes) | `tasks/actor/album.rs` | recomputes `item_count`, `item_size`, `start_time`/`end_time`, cover fallback | n/a |

### Tags, flags, description

| Path / trigger | Handler | Mutates | Guards |
|---|---|---|---|
| Indexing (any path) | `process_image_info`/`process_video_info` → `extract_keywords_from_file` | **stub, always empty** — no tags discovered from file metadata yet (`scenario_n`, red; unit tests in `extract_keywords.rs`, red) | n/a |
| `PUT /put/edit_tag` | `edit_tag.rs` | `object.tags` (add/remove sets) for a batch of indices | `GuardAuth` + `GuardReadOnlyMode` |
| `PUT /put/edit_flags` | `edit_flags.rs` | `is_favorite` / `is_archived` / `is_trashed`, batched; trashing triggers album recount | `GuardAuth` + `GuardReadOnlyMode` |
| `PUT /put/set_user_defined_description` | `edit_description.rs` | `description` for a single index | `GuardShare` (admin **or** a valid share token — broader than most mutating routes) + `GuardReadOnlyMode` |
| `DELETE /delete/delete-data` | `delete_data.rs` | removes records from `DATA_TABLE`, recounts affected albums | `GuardAuth` + `GuardReadOnlyMode` |

### Sharing

| Path / trigger | Handler | Mutates | Guards |
|---|---|---|---|
| `POST /post/create_share` | `create_share.rs` | adds a `Share` to the target album's `share_list` | `GuardAuth` + `GuardReadOnlyMode` |
| `PUT /put/edit_share` | `edit_share.rs` | updates an existing `Share`'s fields (password, expiry, `show_metadata`/`show_download`/`show_upload`) | `GuardAuth` + `GuardReadOnlyMode` |
| `PUT /put/delete_share` | `edit_share.rs` | removes a `Share` | `GuardAuth` + `GuardReadOnlyMode` |
| `GET /share/<path>` (page) + share-scoped reads | `get_page.rs`, `guard_share.rs` | read-only | `GuardShare` (album/share ID + optional password resolved from headers/query/cookie) |

### Read paths (the ones the sidebar/grid actually depend on)

| Path | Handler | Notes |
|---|---|---|
| `POST /get/prefetch?<locate>` | `get_prefetch.rs` | Resolves a filter `Expression` to a `TREE_SNAPSHOT`/`QUERY_SNAPSHOT` pair, named after `Utc::now().timestamp_millis()` / the current `VERSION_COUNT_TIMESTAMP`. See "real bug" note below — this is the entry point for the snapshot-expiry issue in `TODO.md`. |
| `GET /get/get-data?<timestamp>&<start>&<end>` | `get_data.rs` | Per-item payload (the sidepane's data source); requires a `GuardTimestamp` bearer token scoped to `timestamp`. |
| `GET /get/get-rows?<index>&<timestamp>` | `get_data.rs` | Row layout for the virtual grid. |
| `GET /get/get-scroll-bar?<timestamp>` | `get_data.rs` | Scrollbar marks. |
| `GET /get/get-albums` | `get_list.rs` | All albums, flattened with `parentAlbumId`/`dirPath`. |
| `GET /get/get-tags` | `get_list.rs` | Tag → count aggregation. |
| `GET /get/get-export` | `get_export.rs` | Full data export. |
| `GET /object/compressed/<path>`, `GET /object/imported/<path>` | `get_img.rs` | Serves thumbnail/original bytes; gated by **both** `GuardShare` and `GuardHash` (a *third* token type, scoped to one hash via `renew-hash-token`). |
| `GET /get/path-completion?<path>` | `get_fs_completion.rs` | Filesystem autocomplete for the sync-path picker in settings. |

### Auth & config

| Path | Handler | Notes |
|---|---|---|
| `POST /post/authenticate` | `authenticate.rs` | Password → JWT cookie. No-password mode auto-succeeds (what the test harness relies on). |
| `POST /post/renew-hash-token` | `guard_hash.rs` | Mints a `ClaimsHash` token scoped to one hash, from a valid `ClaimsTimestamp`. |
| `POST /post/renew-timestamp-token` | `guard_timestamp.rs` | Renews a timestamp-scoped token, even if expired (`VALIDATION_ALLOW_EXPIRED`) — intentional, for long-lived browser sessions. |
| `PUT /put/config`, `PUT /put/config/password` | `edit_config.rs` | `sync_paths`, `read_only_mode`, `disable_img`, port/limits, password. Changing `sync_paths` reloads the watcher (`reload_watcher`). |
| `GET /get/config`, `GET /get/config/export`, `POST /post/config/import` | `get_config.rs`, `import_config.rs` | Config introspection/backup. |

### Static / page routes

`get_page.rs` (`/`, `/home`, `/albums`, `/<dynamic_album_id>`, the `*/view/<path..>`
variants, `/login`, `/setting`, etc.) just serves the SPA shell or redirects — no data
mutation, no meaningful corner cases beyond routing, intentionally untested per the
philosophy above.

---

## Operation modes

These are the cross-cutting axes that change a handler's behavior independently of its
"happy path" logic. A thorough regression matrix is the *product* of input paths ×
relevant modes, not input paths alone.

| Axis | Values | Where it's checked |
|---|---|---|
| **Admin auth** | no password set (auto-success) / valid JWT cookie / missing or expired JWT | `GuardAuth` (`guard_auth.rs`) |
| **Share auth** | admin JWT / valid share (ID + optional password) via header, query, or cookie / expired or wrong-password share | `GuardShare` (`guard_share.rs`) |
| **Share capability flags** | `show_metadata`, `show_download`, `show_upload` (each independently true/false) | `resolve_show_download_and_metadata` (`operations/mod.rs`), `clear_abstract_data_metadata` (`transitor/mod.rs`), `GuardUpload` |
| **Hash-scoped media token** | valid `ClaimsHash` for *this* hash / valid for a *different* hash / missing | `GuardHash` (`guard_hash.rs`) — gates `/object/*` independently of `GuardShare` |
| **Timestamp-scoped token** | valid `ClaimsTimestamp` matching the query's `timestamp` / mismatched / expired (renewal allowed) | `GuardTimestamp` (`guard_timestamp.rs`) |
| **Read-only mode** | `read_only_mode: true/false` in config | `GuardReadOnlyMode` — gates *every* mutating route; flips to `405` |
| **`sync_paths` configured** | empty (dir-albums feature inert) / non-empty (dir-albums created implicitly on indexing) | `ensure_dir_albums`, `start_watcher.rs` |
| **Presigned album on ingestion** | `None` (watcher/folder-import path — album field never set, `scenario_l`) / `Some(id)` (upload-to-album path — album field *is* set) | `DeduplicateTask::presigned_album_id` |
| **Hash already known** | brand-new content (full `IndexTask` pipeline runs) / hash already in `DATA_TABLE` (short-circuits to `DeduplicateTask`'s merge branch, no re-decode) | `deduplicate_task` (`deduplicate.rs`) |
| **File still at its recorded alias path** | exists (assign/move proceeds) / missing (`assign_album` 4xxs, `scenario_j`) | `assign_album.rs` |
| **Concurrent global version counter** | single in-flight mutation / multiple concurrent indexing-or-edit operations bumping `VERSION_COUNT_TIMESTAMP` | `update_expire_task`, `expire_check_task` — see the snapshot-expiry bug in `TODO.md`; this axis is what made `scenario_o/p/q/s` flaky until `read_current_abstract_data` routed around it |
| **Album type** | filesystem/dir-backed (`dir_path: Some`, path-based membership) / manual (`dir_path: None`, field-based membership) | `generate_filter.rs::Expression::Album`, `combined.rs::belongs_to_album` |

---

## Corner cases and regression test matrix

One row per (input path × operation mode) combination worth pinning down. "Covered by"
names the existing test; "Gap" describes what's not yet tested. This table is meant to
be extended as new modes/paths are added — treat it as the backlog for new
`scenario_*` tests, not a finished checklist.

| Input path | Condition | Expected behavior | Covered by |
|---|---|---|---|
| Watcher indexing | file inside a dir-album's directory | explicit `album` field set to match path membership | ❌ gap — currently fails; red test `scenario_l` pins the *bug*, not the fix |
| Watcher re-index | same hash rediscovered at its *current* alias path (e.g. after `assign_album`) | `alias` unchanged (no duplicate) | ❌ gap — red test `scenario_m` |
| Watcher re-index | same hash rediscovered at a *new* path (file moved externally) | dead alias entries pruned | ❌ gap — red test `scenario_r` |
| Watcher indexing | file with embedded XMP/IPTC keywords | keywords become tags | ❌ gap — red tests `scenario_n` + `extract_keywords::tests::*`; needs per-format coverage (see `TODO.md`) |
| `POST /upload?presigned_album_id_opt=X` | new hash | `album` field set to `X` at index time | 🟡 untested — only the redb-level logic in `deduplicate_task` is read, not exercised end-to-end |
| `POST /upload?presigned_album_id_opt=X` | hash already exists with a *different* album | last-write-wins (`set_album` overwrites) | 🟡 untested — plausible-acceptable behavior per code reading, not asserted |
| `PUT /put/assign_album` | target album exists, file present, different from current album | file moved, `album` field updated, both albums' stats refreshed, change visible via `/get/get-data` | ✅ `scenario_h`, `scenario_i`, `scenario_q` |
| `PUT /put/assign_album` | file missing at recorded alias path | 4xx, DB unchanged | ✅ `scenario_j` |
| `PUT /put/assign_album` | target is a manual (non-dir) album (no `dir_path`) | currently **always** 4xxs (`get_dir_path_for_album` returns `None`) — assign_album cannot target manual albums at all | ❌ gap — not asserted anywhere; worth confirming this is intentional product behavior, not an oversight |
| `POST /put/reindex` | item has a previously-assigned album and tags | both survive the reindex | ✅ `scenario_s` |
| `PUT /put/edit_tag` | add/remove a tag via the real prefetch → edit → get-data round trip | tag visible afterwards | ✅ `scenario_p` |
| `GET /get/get-data` | item with tags set via fixture/indexing | tags visible via the same field the sidebar reads | ✅ `scenario_o` |
| `GET /get/get-data`, `/get/get-rows`, `/get/prefetch` | concurrent version-counter bumps between `prefetch` and the read | snapshot should live ~1 hour, not vanish immediately | ❌ **known bug**, not fixed — see `TODO.md` "prefetch snapshots can be deleted almost immediately" |
| `GET /object/*` | valid `GuardShare` but missing/wrong-hash `GuardHash` | 401/403, not served | 🟡 untested |
| `GuardReadOnlyMode` | `read_only_mode: true`, any mutating route | 405 | 🟡 untested at the route level (logic itself is a one-line config check) |
| Share read (`GuardShare`) | `show_metadata: false` | `album`, `tags`, `alias`, `exif_vec` stripped from the response (`clear_abstract_data_metadata`) | 🟡 untested |
| Share read | `show_download: false` | download token/URL omitted | 🟡 untested |
| `POST /upload` via share | `show_upload: false` vs `true` | upload rejected vs accepted | 🟡 untested |
| Dir-album nesting | photo several levels deep under nested dir-albums, once `scenario_l`'s bug is fixed | explicit `album` resolves to the **deepest** matching directory, not an ancestor | ❌ gap — flag for follow-up once the fix lands; `scenario_d`/`scenario_e` already cover the *path-based* membership side of nesting, just not the (currently nonexistent) explicit-field side |
| Video pipeline | same XMP-keyword discovery as images | parity with image pipeline (now wired, still stubbed) | ❌ gap — needs `ffmpeg`/`ffprobe` available in the test env; see `TODO.md` format-coverage item |

---

## Backend

### Unit tests

Target: pure functions with no I/O, no DB, no server. These live in `#[cfg(test)]` blocks
in the same file as the code under test and run with `cargo test` / `cargo nextest`.

Priority targets:

| Function | File | Why |
|---|---|---|
| `prettify_dir_name` | `dir_album.rs` | Pure string transform; edge cases around separators, casing, unicode |
| Schema version dispatch | `ser_de.rs` | Silent corruption if the `[0xFF, version]` prefix logic regresses; encode/decode round-trips per version |
| `Expression` filter predicates | `generate_filter.rs` | Filters are composed at runtime; incorrect predicate logic silently returns wrong results |
| `compute_timestamp` | `abstract_data.rs` | Priority logic across EXIF, file, and fallback timestamps |
| `belongs_to_album` path-prefix branch | `combined.rs` | The dir-vs-manual discriminator; path-prefix semantics are subtle |

### Integration tests

Target: multi-component flows where the types don't constrain the interaction.
redb uses a single embedded file, so a test can spin up a real DB in a `tempdir` — no
mocking needed, and no separate process required.

Priority flows:

- Index file → deduplication → `FlushTreeTask` → album `self_update` round-trip
- Dir album creation and path-prefix membership (file inside subtree vs. outside)
- Schema migration: write a v1-encoded record directly (raw bytes), read it back through
  `from_bytes`, verify the promoted `dir_path: None` field is present

### Tooling gaps

- **cargo audit**: CVE scan of the dependency tree. Run locally and in CI.
- **clippy `unwrap_used`**: set to `warn` in `Cargo.toml` (visible in IDEs and plain
  `cargo clippy`); excluded from the precommit `-D warnings` flag until the ~140 existing
  call sites are addressed.
- **cargo geiger**: counts unsafe blocks in the crate and all dependencies. Not suitable
  for precommit (slow, requires full build); include in periodic release/audit reports to
  track the unsafe surface area of the dependency tree.

---

## Frontend

### Unit tests — Vitest

The lexer (`src/script/lexer/`) is the highest-value first target: it has a hand-written
grammar (Chevrotain), is pure logic with no DOM dependency, and is complex enough that
regressions are non-obvious. Pinia store reducers are the next target — pure state
transforms, no component rendering needed.

Vitest integrates with the existing Vite config with minimal setup (`vite.config.ts`
already present).

### Dead code — knip

Run `npx knip` periodically to find unused exports and imports. Not suitable for
precommit (generates noise on in-progress refactors); better as an occasional manual
sweep or CI step.

### Security — npm audit

`npm audit` should run in CI on every PR. Not in precommit: too slow, and the existing
`overrides` in `package.json` mean some findings are known and intentional.

### E2E — Playwright (deferred)

Requires a running backend, so not practical in precommit. Intended for CI only.
Golden-path smoke tests: login → gallery view → open image → share link flow.

---

## What is not tested and why

| Area | Reason |
|---|---|
| Rocket route definitions | Framework handles dispatch; type-checked at compile time |
| redb read/write primitives | Covered by redb's own test suite |
| bitcode round-trips on unchanged structs | Covered by bitcode's own test suite; only the *version dispatch logic* needs testing |
| Vue component rendering | Deferred to E2E; unit-testing rendering adds maintenance cost without proportional value |
| Individual Pinia getters backed by a single field | No logic to test; the type system covers it |
