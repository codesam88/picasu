# Local backlog (gitignored)

## General items, not a priority

- standalone TLS deployment path: add `tls` Cargo feature gating `rocket/tls`, validate end-to-end with a real cert (certbot/acme.sh), document in CONFIG.md; only ship once tested
- review for license issues, SPDX label and OSSF score card
- add a proper documentation / generation system
- move the fork to github, compatible CI flows and Android app by Ted(?)
- enable the release/deployment flows in CI
- add android app 
- review code module by module

## Robustness / Assurance

- wrap image repo file operations and DB updates into a single transaction
  - the idea is to extend the DB consistency assurance to the photo repo
  - similar to journaling filesystems, a marker (mutex? file log) should record the planned transaction, perform the DB update, then file update, then release the transaction lock
  - if we crash before modifying files, we can detect the unfinished transaction and either rollback or complete, thus ensuring consistency
  - maybe there are better patterns and tooling to do this efficiently. but it should work on any (local) filesystem

## CI

- [x] resolved: 'just build'/'just run'/'just test' now use a debug build without embed-frontend
  (the developer default, matching check/test); 'just build-release' is the production
  configuration (release + embed-frontend), matching CI's release workflow and the installer
  scripts. See `docs/test-strategy.md` "Build configurations: developer vs. production".
- [ ] CI should be expanded to test all supported configurations on PR and release:
  - PR/merge CI: developer config (debug, no embed-frontend) — matches local precommit
  - release CI: production config (release + embed-frontend) — already done in
    `.github/workflows/release.yml`, but not yet gated as a required PR check before release

- tags assigned via api should be merged into DB as well as image repository on disk (idea borrowed from Immich)
  1. use xmp sidecar files to store all extracted data without modifying originals
  2. on indexing, if no xmp sidecar exists, create from image metadata
  3. on tag or other metadata changes, first store to db and then to xmp sidecar
  4. on album assignment (causing the original file to be moved), xmp sidecar files are moved together with the originals 
  5. we can provide a separate helper script to merge metadata from sidecar files back into originals. major photo apps probably also support such files.
  - DEFERRED — not part of the storage-architecture fix below; xmp sidecars are a separate
    future step.


## Github fork

- [ ] work on github fork for more compatible CI + contributing back
- [ ] push all changes to a dev branch
- [ ] refactor changes to first apply devops, tests, audit fixes, then merge / compress the album stuff?

## Docker / Deployment

- [ ] **Verify the rework actually works** — no docker daemon in the dev environment this was
  written in, so none of the above has been build- or run-tested. Before relying on it: `docker
  compose build`, confirm the image builds and `embed-frontend` actually picks up the frontend
  dist copied into the builder stage; `docker compose up`, confirm config/data/images land in the
  bind-mounted host dirs and the app is reachable; confirm `imagePath` set to `.` (or any relative
  value) resolves against `/images` inside the container as expected.
- [ ] Publish to `ghcr.io/codesam/urocissa` (GitHub Container Registry) — free for public images,
  co-located with source; complement or replace Docker Hub `hsa00000/urocissa`, which this fork
  can't push to.
- [ ] Add a systemd `.service` file in `deploy/` or `contrib/` for users running the binary
  directly (non-Docker), now that storage locations are env-var-driven
  (`UROCISSA_CONFIG_HOME`/`UROCISSA_DATA_HOME`/`UROCISSA_IMAGE_HOME` set in the unit file rather
  than relying on autodetection).
- [ ] `UROCISSA_STATE_HOME`: `UROCISSA_DATA_HOME` holds `db/index_v5.redb` (irreplaceable —
  the only store of record for tags/album-assignments/flags) alongside genuinely disposable
  cache files (`db/cache_db.redb`, `db/temp_db.redb`, `db/expire_db.redb`). Splitting the latter
  into a proper state directory would make "what's safe to delete on reset" explicit instead of
  implicit in file names. Bigger change than the env var rename — touches each redb file's path
  individually (`tree_snapshot/new.rs`, `query_snapshot/new.rs`, `expire/new.rs`). Not done yet.
  (`object/imported/` and `upload/`, previously listed here, no longer exist — see "Storage
  architecture fix" above.)

---

## Code conventions and tooling notes

### Pre-commit hook order
`just precommit` runs in this order: `cargo fmt --check` → `cargo clippy` → `cargo nextest` → `prettier --check` → `vue-tsc + eslint` → `vitest`. Run `cargo fmt` and `npx prettier --write` before staging to avoid the most common late failures. ESLint and vue-tsc run after prettier, so formatting errors mask type/lint errors until fixed.

### Error code policy (backend)
`ErrorKind::NotFound` maps to HTTP 404, which Rocket also returns for unregistered routes. Using `NotFound` for domain-level "entity not found" makes those errors indistinguishable from routing failures in tests and clients. Use `ErrorKind::InvalidInput` (→ 400) when the route succeeded but a referenced entity does not exist. Only use `NotFound` when a routing-level 404 is genuinely the right signal.

### ESLint strict mode (frontend)
`@typescript-eslint/strict-boolean-expressions` and `@typescript-eslint/no-non-null-assertion` are enforced. These only surface at commit time (pre-commit hook). When writing TypeScript: use `=== null` / `=== undefined` instead of truthiness checks on nullable strings; avoid `!` non-null assertions — use an explicit null/undefined guard branch instead.

---

## Album feature — open items

- [ ] **E2E test for `create_dir_album`** — endpoint has no scenario coverage. Should verify: subdir is created on disk, returns new album ID, parent album updates.
- [ ] **Stale `DIR_ALBUM_CACHE` scenario** — if a directory is deleted externally while the cache still holds its entry, `assign_album` will attempt to move a file into a non-existent path. Decide: detect and evict stale entries at startup, or return a clear 400 with a meaningful message at request time.
- [ ] **`assign_album` file-move verification in tests** — Scenario H checks album membership after assignment but does not verify the file moved to the correct directory on disk. Add a filesystem assertion to close this gap.

---

## Robustness: FS/DB consistency reporting (planning — not yet implemented)

Status: **plan only**. No code changes yet. Once agreed, implementation should add a
"Robustness strategy" section to `docs/design.md` documenting the pattern below (not
the survey/rationale — that stays here), and extend `docs/test-strategy.md`'s
regression matrix with rows for the hook points listed.

### Problem

`gallery-backend` treats the filesystem and redb as two systems that are supposed to
stay in sync (file paths recorded in `alias`/`dir_path` vs. what's actually on disk),
but nothing currently watches for them drifting apart except the narrow fixes already
made for `alias` pruning (see "Known bugs", now fixed). Drift happens for ordinary
reasons — files edited/moved/deleted outside the app, races between the watcher's
debounce window and a concurrent edit, directories removed while their dir-album
record still exists — and today the system's response to each is inconsistent: some
cases self-heal silently, some panic, some become an opaque 500 with no indication
that the *cause* was a consistency problem rather than a generic bug, and none of them
are visible to the user in a way that distinguishes "this still worked, but you should
know X" from "this failed because of X."

### Survey of concrete situations

| # | Location | Scenario | Today | Should be |
|---|---|---|---|---|
| 1 | `deduplicate_task` (`tasks/actor/deduplicate.rs`) | Rediscovering a hash at a path that doesn't match any *other* existing record, but that record's `alias` already had dead entries pruned this run (just-fixed behavior) | Silent (now correct, but no log line distinguishes "pruned a stale entry" from "nothing to prune") | `info!` log noting how many dead aliases were dropped — expected/self-healing, not Discord-noised |
| 2 | `deduplicate_task` | A file's content changes *in place* at an already-tracked path (new hash discovered at a path some *other* existing record's `alias` still references) — that other record now has a dangling alias nobody prunes, since pruning only happens on the *same* hash's own re-discovery | Not handled at all — silent DB drift, two records can claim the same physical path | New: cross-record stale-alias detection when indexing a brand-new hash; `warn!` + prune the stale reference in the old record |
| 3 | `assign_album.rs` | `get_dir_path_for_album` returns `None` because the target album's directory was deleted externally after the album record was created (stale `DIR_ALBUM_CACHE` entry — already flagged above) | Generic `ErrorKind::InvalidInput` 400, "Album not found in dir cache" | Distinguish "never was a dir album" (genuine bad request) from "was one, directory now missing" (consistency problem) — different message, `warn!` logged server-side either way |
| 4 | `assign_album.rs` | `fs::rename` fails for a reason other than the already-checked "source missing" (e.g. `EXDEV` cross-device, permission denied, destination collision) | Generic `ErrorKind::Internal` 500 | Classify by `io::ErrorKind`; cross-device/permission issues are operator-fixable, not "internal bug" — should be 4xx-ish with an actionable message |
| 5 | `workflow::index_for_watch` → `OpenFileTask` | File is deleted between the watcher's `Create` event firing and the (debounced + queued) indexing attempt running — a normal race for *any* filesystem watcher, not a bug | After 3 retries, hard error → `handle_error` → Discord webhook noise | Recognize `ErrorKind::NotFound`-shaped IO errors here specifically and downgrade to `info!`/`debug!`, no Discord notification — this is expected, not exceptional |
| 6 | `tasks/actor/album.rs::album_task` | A dir-album's `dir_path` directory is deleted externally; the album record has no live members but is never cleaned up, and `DIR_ALBUM_CACHE` keeps pointing at a dead path forever (same root cause as #3) | Silent — album just sits at `item_count: 0` indefinitely | `warn!` once detected (e.g. during `album_task` or a periodic sweep) that a dir-album's directory no longer exists; surface in `/get/get-albums` as a flag the frontend can badge, rather than only failing later when something tries to use it (#3) |
| 7 | `router/put/reindex.rs:50` | `reduced_data_vec.get_hash(*index).unwrap()` — panics if `index` is out of range for the snapshot (e.g. the snapshot is stale relative to a concurrent delete) | `.unwrap()` panic — Rocket likely converts it to a generic 500 with no structured logging at all | Convert to `or_raise(ErrorKind::Conflict, ...)` ("snapshot is stale, re-fetch and retry") — ties into the broader effort to retire `.unwrap()` call sites (see "Backend tooling" below) |
| 8 | Any two concurrent read-modify-write handlers on the same hash (e.g. `edit_tag` + `rotate_image` + the watcher's own re-index, all racing) | Each handler reads the full `AbstractData`, mutates one part, writes the whole thing back — classic lost-update: B's write can silently discard A's concurrent change | Not detected at all; redb's transaction isolation prevents *corruption* but not *lost updates* | Out of scope for the immediate pattern (bigger change — would need optimistic concurrency, e.g. an `updated_at`/version check on write, or per-hash locking extended from indexing's existing `IN_PROGRESS` guard to API handlers). Flag here so it isn't forgotten; not a "warning to surface," a correctness gap. |
| 9 | `get_prefetch.rs` / `expire_check.rs` (the snapshot-expiry bug, "Known bugs" above) | A `prefetch` snapshot is deleted within milliseconds instead of living ~1 hour, so the next `/get/get-data` 500s with "Failed to open tree snapshot table" | Hard 500, `ErrorKind::Database`, no indication this is a known/transient consistency issue vs an unrelated DB problem | Good first real consumer of this pattern once it exists: reclassify as a distinct, recoverable kind the frontend can react to by silently retrying (it already knows how — `prefetch` is idempotent), rather than just displaying a raw error |
| 10 | General | ~140 existing `clippy::unwrap_used` call sites across the backend (tracked separately under "Backend tooling") | Each is a potential ungraceful panic on exactly this kind of unexpected state | Treat consistency-pattern adoption and `unwrap_used` cleanup as overlapping efforts — most of the call sites worth fixing first are the ones touching redb lookups / filesystem state, i.e. exactly this list |

### Classification

Every situation above falls into one of three buckets; the pattern needs to make the
bucket explicit in code, not just in a comment:

1. **Expected, self-healing** (#1, and the alias-pruning/dead-watcher-race fixes already
   shipped) — the operation completes normally; log at `info!`/`debug!` for
   observability, never surfaced to the user, never Discord-noised.
2. **Unexpected but recoverable** (#2, #3 "was a dir album", #6, #9) — the operation
   either completes with a caveat or fails in a way the user can act on (re-index,
   re-pick a target, retry). Needs both a structured server-side log (`warn!`,
   Discord-noised — this is the kind of thing that indicates real drift worth a human
   noticing) *and* a way for the relevant API response to say "warning: X" rather than
   either silent success or an opaque error.
3. **Unexpected and blocking** (#4, #5 non-NotFound branch, #7) — the operation cannot
   complete. Already representable via `AppError`/`ErrorKind`; the gap is precision
   (today these mostly collapse into `Internal`) and consistent logging discipline
   (some go through `handle_error`, some don't, with no obvious rule for which).

### Proposed backend pattern

- Add `ErrorKind::Inconsistency` for bucket 3 cases that are specifically *FS/DB
  drift*, not generic bugs — lets the frontend branch on `kind` to show a different
  message/icon than for, say, `ErrorKind::InvalidInput`. Maps to a 409 or 500
  depending on the case; decide per call site.
- For bucket 2, the open design question (flag for review, don't decide unilaterally
  here): either (a) extend success responses with an optional sibling field —
  e.g. wrap relevant endpoints' `AppResult<Json<T>>` in a small
  `Json<WithWarnings<T>> { data: T, warnings: Vec<String> }` envelope only where
  needed, keeping today's plain `T` for everything else — or (b) keep responses as
  today and rely purely on server-side logging + a *separate* lightweight polling/log
  surface for admins, accepting that end users won't see bucket-2 warnings inline.
  (a) is more work but is what "the user dialog should also report this" in practice
  requires for the album/tag-assignment flows specifically called out below.
- A single logging helper (next to `handle_error`/`handle_app_error` in
  `public/error_data.rs`) — e.g. `log_consistency(severity, context: &str, detail:
  impl Display)` — so every hook point uses the same call shape and the
  Discord-webhook-or-not decision lives in one place (mirroring the existing
  `handle_app_error` filter-by-`ErrorKind` logic), instead of each call site deciding
  ad hoc.

### Proposed frontend pattern

- Add `'warning'` to `MessageColor`/`messageStore.ts` (currently `error` / `success` /
  `info` only) and a `messageStore.warning(text)` action — Vuetify already has a
  `warning` theme color, so this is additive, not a new design.
- For the two flows the request calls out by name — `AssignAlbumModal.vue` (album
  assignment) and the tag editor (`PUT /put/edit_tag` call sites) — branch on the
  response: hard failure → `messageStore.error` (already happens); success with a
  `warnings` array (once the backend envelope from the bullet above exists) →
  `messageStore.warning` for each entry, *in addition to* showing the operation as
  completed (don't suppress the success state, since bucket 2 means it *did* succeed).
- Longer term, the same envelope/branching should apply to any other mutating call
  site that can hit a bucket-2 situation (reindex, delete, rotate) — start with
  album/tag assignment since that's what surfaced this need, generalize once the
  pattern is proven there.

### Sequencing (for whenever this gets implemented — not now)

1. Backend: add `ErrorKind::Inconsistency`, the `log_consistency` helper, and the
   `WithWarnings<T>` envelope (or the chosen alternative from the open question above).
2. Wire hooks #1–#3, #5, #6, #9 from the survey table (the ones that are pure
   classification/logging changes, no new detection logic needed).
3. Add the cross-record stale-alias detection for #2 (the one genuinely new piece of
   logic).
4. Frontend: add the `warning` message color/action; wire `AssignAlbumModal.vue` and
   the tag editor to surface `warnings` from responses that carry them.
5. Document the resulting pattern in `docs/design.md` (a new "Robustness" subsection,
   alongside "Keep it robust, low footprint, modular") and add the corresponding rows
   to `docs/test-strategy.md`'s regression matrix, with one `scenario_*` test per hook
   point in the survey table (most can reuse the existing fixture techniques — delete
   a file mid-flow, point a dir-album at a missing directory, etc.).
6. Re-fix #9 (the snapshot-expiry bug) using the new pattern as the worked example,
   instead of the current hard 500.
7. #8 (lost updates under concurrent writes) is explicitly *not* in this sequence —
   it needs its own design discussion (optimistic concurrency vs. locking) and
   shouldn't block the consistency-reporting pattern above.

---

## Testing — best value items

### Backend integration tests (need tempdir redb — not as hard as it sounds)
- [ ] index → dedup → flush → album self-update round-trip
- [ ] dir album path-prefix membership (create album, check file in/out of subtree)

- [ ] semi-regular security audit CI job — run `just audit` on a schedule (monthly or on dep changes); options: Codeberg CI (Woodpecker), GitHub Actions on the upstream fork, or a local cron on the dev machine; the `/security-audit` skill covers the manual review workflow when issues are found

### Frontend unit tests
- [ ] Pinia store reducers — pure logic in stores, no DOM needed

### Frontend tooling
- [ ] knip — periodic dead-export sweep, not in precommit
- [ ] dependency updates — establish a regular cadence (npm-check-updates, Dependabot, or Renovate); small frequent updates are cheaper than batched drift
- [ ] Zod (or valibot) — investigate for runtime validation of API responses; TypeScript types don't catch backend schema drift at runtime

### Known bugs
- [ ] **Grey placeholders on initial load** — `useElementSize` reports width 0 on first render; `usePrefetch` skips the fetch (guard: `windowWidth > 0`); the Home.vue watch then fires a row fetch before `prefetch()` sets the correct timestamp, so rows arrive stale and are discarded. A zoom or resize re-triggers the cycle cleanly and images appear. Root: `useElementSize` ResizeObserver fires after the first render tick, too late for the initial fetch path. Fix: ensure `prefetchStore.windowWidth` is set before the first `fetchRowInWorker` call, or delay the Home.vue watch fetch until after `processPrefetchChain` completes.
- [ ] **`prefetch` snapshots can be deleted almost immediately instead of living for 1 hour** — found while stabilising the new e2e tests (was causing real, intermittent 500s on `/get/get-data` right after `/get/prefetch`, not just test flakiness). `Expire::expired_check` (`public/db/expire/expired_check.rs:77`, `None => true`) cannot distinguish "this timestamp was never recorded" from "this is the *current* `VERSION_COUNT_TIMESTAMP`, whose expiry hasn't been scheduled yet" (`update_expire_task` inserts `expire_table[current_timestamp] = None` deliberately, as a placeholder). Both collapse to the same `None` arm. Since `flush_query_snapshot_task` names each on-disk `QUERY_SNAPSHOT` table after the *current* `VERSION_COUNT_TIMESTAMP` at flush time, the very next version bump anywhere in the process (any concurrent indexing/edit/reindex) makes `expire_check_task` see `VERSION_COUNT_TIMESTAMP > that_table's_timestamp` and call `expired_check` on it — which returns `true` immediately via the `None` arm instead of waiting ~1 hour, deleting the query snapshot table and cascading (via the `RemoveTask` it schedules for every `Prefetch.timestamp` row inside) to delete the just-created `TREE_SNAPSHOT` table too. Net effect: under any concurrent write activity, a freshly-`prefetch`'d snapshot can vanish within milliseconds, and the next `/get/get-data`/`/get/get-rows` call against it 500s with "Failed to open tree snapshot table". Worked around in tests via `read_current_abstract_data` (re-prefetch-and-retry instead of reusing a timestamp); not worked around in the frontend. Fix: distinguish "unrecorded" from "active, not yet scheduled" — e.g. store expiry as a tri-state, or only ever insert the *current* version's row once its *own* successor exists, or check `entry.is_none()` separately from "row absent" before defaulting to "expired".
- [ ] **Enable metadata-extraction tests for all supported file formats** — `extract_keywords_from_xmp`'s current (stub) substring-scan approach is format-agnostic by construction, but only `scenario_n` (JPEG, hand-spliced APP1/XMP segment) exercises it end-to-end. Supported extensions today (`public/constant/mod.rs`): images `jpg, jpeg, jfif, jpe, png, tif, tiff, webp, bmp`; videos `gif, mp4, webm, mkv, mov, avi, flv, wmv, mpeg`. XMP/IPTC packet location and encoding differ by container — PNG can store text in compressed `zTXt` chunks (the naive scan would miss it), MP4/MOV embed XMP in a `uuid` box, TIFF has no `APPn` segment concept, and IPTC IIM (the older, non-XMP keyword mechanism some tools still write) is a separate binary format entirely. Once real extraction is implemented, extend coverage with one `scenario_n`-style test per representative container (at least: PNG with an uncompressed `iTXt` XMP packet, PNG with a compressed `zTXt` one, MP4 with a `uuid` XMP box, and a plain IPTC-IIM-only JPEG) rather than assuming the JPEG case generalises. Video-pipeline coverage additionally needs `ffmpeg`/`ffprobe` available in the test environment (process_video_info shells out to both), which `scenario_n`'s image-only path avoids.

### Deferred
- [ ] Playwright E2E — needs running backend; save for CI
- [ ] cargo bench / criterion — indexing pipeline regression benchmarks (file hashing, EXIF extraction, thumbnail generation, redb write throughput, serving latency under load)
- [ ] Admin status API — expose queue depth, active indexing jobs, recent errors, per-job progress as a JSON endpoint (foundation for a future admin panel)
- [ ] Frontend progress UI — poll the existing `/get/import/folder/status` pattern (or a future SSE stream) to show active indexing progress to authenticated users
- [ ] Structured logging — investigate `tracing` + `tracing-subscriber` as a replacement for `env_logger` to support spans, JSON output for log aggregators, and `tracing-journald` for native systemd/journald integration

## Spec-driven E2E testing (planning — not yet implemented)

Goal: generate E2E test code from a semi-formal spec rather than hand-writing
(or AI-writing) each test body. Two layers: a **backend-authoritative interface
contract** (Rust `utoipa` annotations → `openapi.json`) fixes operation names and
request/response schemas; a **scenario DSL** (Given/When/Then YAML) describes
fixtures, calls, and assertions. A standalone generator expands scenarios into
the existing `e2e.rs` Rocket-`Client` test style, and later into Playwright specs.

Confines AI/manual interpretation to scenario *authoring*; inputs, serialization,
and assertions are mechanical. See `docs/test-strategy.md` for the broader testing
picture this slots into.

Decisions taken:
- **Standalone tool via cargo xtask**, not a build script — keeps `cargo test`
  fast, debuggable, and leaves room for further generators/tooling under one entry.
- **Backend-authoritative contract** (`utoipa`) — no FE/BE drift window; Rust owns
  the schema as it owns the data (redb/bitcode).
- **Scenario-DSL-first** — the first spike targets stateful flows (the source of
  most real bugs), with the contract layer as the operation registry + type-check
  for scenario bodies.

### Prerequisite: Cargo workspace + xtask

- [ ] Convert repo to a Cargo workspace. Today `gallery-backend/Cargo.toml` is a
  bare `[package]` with no root manifest. Add a root (or `gallery-backend`-level)
  `[workspace]` with members: the existing `urocissa` crate and a new `xtask` crate.
- [ ] `xtask` crate: subcommand dispatch (`xtask gen-scenarios`, `xtask emit-openapi`,
  room for `xtask check-coverage`). Alias `cargo xtask` via `.cargo/config.toml`.
- [ ] Add `just gen-scenarios` / `just emit-openapi` recipes wrapping the xtask calls;
  wire generation + the coverage gate into `just precommit` so generated code and
  spec cannot drift from source unnoticed.

### Interface contract (utoipa, backend-authoritative)

- [ ] Add `utoipa` dependency. Annotate the 3 spike endpoints first
  (`POST /post/authenticate`, `GET /get/get-albums`, `PUT /put/assign_album`) with
  path + request/response schema derives.
- [ ] `xtask emit-openapi` writes `gallery-backend/openapi.json` (committed, reviewable).
  A test asserts the emitted doc matches the committed file (drift guard).
- [ ] **Coverage reporting**: introspect the mounted `routes!` set vs OpenAPI paths;
  emit a coverage percentage and list unannotated routes. Report in CI output but
  do not fail the build — annotation is incremental, not a gate.
- [ ] Backfill `utoipa` across remaining routes incrementally, endpoint-by-endpoint.

### Refactor e2e.rs: fixtures vs. test code

- [ ] Split `src/tests/e2e.rs`. Move reusable fixture builders/helpers
  (`make_dir_album`, `insert_photo_with_real_file`, `make_client`, `auth_cookie`,
  `refresh_in_memory`, the `TestEnv`/`DATA_PATH` setup, redb read helpers) into a
  `src/tests/fixtures.rs` (or `tests/support/`) module with stable public signatures.
- [ ] These fixtures become the DSL `given:` vocabulary primitives — the generator
  emits calls to them, so their signatures are an API the generator depends on.
- [ ] Leave the existing hand-written scenarios in place initially; generated
  scenarios live in a separate `src/tests/scenarios_generated.rs` (clearly marked
  generated, never hand-edited). Port scenarios to the DSL one at a time.

### Scenario DSL: spec + documentation (in docs/)

- [ ] Write `docs/scenario-dsl.md`: the semi-formal spec + authoring guide. Must
  define, with examples, the fixed vocabulary:
  - `given:` — `dir_album`, `photo`, `tag`, `empty` (each maps to a fixture builder);
    `id_as` binding syntax for referencing created entities (`$album`, `$photo`).
  - `when:` — `call: <operation>` (name resolved against `openapi.json`; `body`
    validated against the operation's request schema at generation time); auth and
    content-type derived from the contract.
  - `then:` — `response.status`, `response.<json-path>`, `file_exists`/`file_absent`,
    `db.image(...)/album(...)/tag(...).<field>` assertion forms; each maps to exactly
    one generated assertion.
  - Escape-hatch policy: **no raw-Rust escape hatch**. A missing assertion form is
    resolved by adding a reusable verb to the vocabulary, not by inlining code.
- [ ] Define the DSL's own schema (JSON Schema for the YAML) so scenario files are
  validated structurally before generation, independent of the contract check.
- [ ] Reference `docs/scenario-dsl.md` from `docs/test-strategy.md`.

### Generator (xtask gen-scenarios)

- [ ] Parse `scenarios/*.yaml`, load `openapi.json`, validate each scenario:
  structural (DSL schema) + contract (operation exists, body matches request schema).
  Fail generation on any violation — bad specs never produce green tests.
- [ ] Emit one Rust test fn per scenario into `scenarios_generated.rs`, in the
  existing Rocket-`Client` style, calling the refactored fixture helpers.
- [ ] Port Scenario H (`assign_album moves file and updates membership`) as the
  reference scenario; assert the generated test is behaviourally equivalent to the
  hand-written one before deleting the latter.
- [ ] **Trust guard — mutation testing**: run `cargo-mutants` against the handlers a
  scenario covers; a generated suite that stays green under a mutated handler has
  weak assertions. Track as a periodic check, not necessarily per-commit.

### Playwright extension (full-stack, after the backend loop is proven)

Scenarios carry a top-level `target: api | ui` field. Existing scenarios are
implicitly `target: api`. The generator dispatches on this field; both targets
read the same `openapi.json` for operation resolution.

**`target: api` (current plan — unchanged)**
Expands into Rocket-`Client` Rust tests. `given:` seeds redb directly; `when:`
is an API call; `then:` asserts response fields and redb/filesystem state.

**`target: ui` (new)**
Expands into Playwright TypeScript specs driving a real backend + built frontend.

- [ ] `given:` seeds state via a minimal API seeding layer (not redb-direct) that
  is kept separate from the handlers under test. This avoids the circular failure
  mode where a broken handler corrupts both fixture and assertion simultaneously;
  the seeding layer only calls endpoints that are not the subject of the scenario.
- [ ] `when:` describes browser user actions. Elements are referenced by ARIA role
  and accessible name, not CSS selectors, so specs don't couple to DOM structure.
  Fixed verb set:
  - `navigate: <route>` — go to a URL pattern
  - `click: <role>/<label>` — click by ARIA role + accessible name
  - `fill: <role>/<label>, value: <value>` — type into an input
  - `select: <role>/<label>, option: <label>` — choose from listbox/select
  - `submit:` — submit the current form
  New interactions → extend the vocabulary with a new verb; no raw-TypeScript
  escape hatch. A missing verb is a gap in the DSL, not a reason to inline code.
- [ ] `then:` UI assertions:
  - `ui.visible: <role>/<label>` / `ui.hidden: <role>/<label>`
  - `ui.text: <role>/<label>, contains: <text>`
  - `ui.toast: type: <error|success|warning>, contains: <text>`
  - `ui.modal: open | closed`
  - `ui.route: <pattern>` — current URL matches
  - `ui.aria_snapshot: <name>` — compare the ARIA role/name/state tree of a named
    page region against a committed `.aria` snapshot file; regenerate intentionally
    with `xtask update-snapshots`.

**Frontend element and layout spec**

Playwright's `toMatchAriaSnapshot()` is the mechanism for specifying UI structure:
it serialises the accessible role/name/state tree of a component or page region
and diffs it against a committed snapshot. This covers semantic layout — element
presence, ordering, labelling, nesting — without pixel-level fragility.

- [ ] `ui.aria_snapshot` verbs in `target: ui` scenarios are the spec-first path
  for new UI: write a scenario with an `aria_snapshot` assertion before the Vue
  component exists; the generated Playwright spec fails until the component matches.
  This is what "specifying the frontend interface" means in practice.
- [ ] Pixel-level visual regression (`toHaveScreenshot()`) is environment-sensitive
  (font rendering, GPU, OS). Treat as an optional separate layer if needed, not
  part of the core spec DSL.
- [ ] `ui` scenario files live in `scenarios/ui/*.yaml`; generated specs in
  `tests/playwright/scenarios_generated/`. Snapshots committed at
  `tests/playwright/snapshots/`.

**Process management**

- [ ] Spawn backend on an ephemeral port with a tempdir `DATA_PATH`; build frontend
  once per suite run; tear down after. Gate on CI (matches the existing "Playwright
  E2E — needs running backend; save for CI" deferral above).

### Sequencing

1. Workspace + xtask skeleton; `emit-openapi` for the 3 spike endpoints.
2. Refactor `e2e.rs` to split fixtures from test code.
3. Write `docs/scenario-dsl.md` + the DSL JSON Schema; include the `target:` field,
   the `ui` `when:` verb set, `then:` UI assertion forms, and the escape-hatch policy
   for both targets.
4. Generator: validate + emit `target: api`; port Scenario H; review generated vs hand-written.
5. Coverage reporting + `cargo-mutants` wiring.
6. Backfill `utoipa` across all routes; port remaining `api` scenarios to the DSL.
7. Playwright target: implement `target: ui` generator; wire process management and
   the API seeding layer; port Scenario H as a `target: ui` scenario as reference.
8. Write `ui.aria_snapshot` specs for the 3 spike UI flows — album assignment modal,
   tag editor, message toast area — as spec-first drivers for those components.
9. Extend UI DSL vocabulary as new interaction patterns arise; update
   `docs/scenario-dsl.md` for each new verb added.
