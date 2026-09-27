---
status: open
type: feature
priority: high
area: backend
---

## Notes

Audit of `docs/openapi-reference.md` (widdershins output) against
`backend/openapi.json`, the utoipa annotations, and the actual Rocket routes.
Route coverage itself is complete (apparent gaps were parser artifacts of
multi-line attributes and Rocket `{x..}` segments); the problems are in the
contract content, organization, and rendering. No work started yet.

## Problem and Hardening Strategy

The exposed API can drift even when a focused rework follows the intended
path-primary design and removes known migration artifacts. The recent review
found stale naming, incorrect identifier values at live call sites, missing
OpenAPI operations, incomplete authentication responses, and documentation
that rendered incorrectly. These defects were not caught by the existing API
or Playwright scenarios because those tests cover selected workflows rather
than the complete public contract. The generated OpenAPI document also did
not prevent drift: route registration, annotations, generated paths, and
public-spec filtering are separate sources of truth.

The first five hardening mechanisms should be established as recurring CI
checks and treated as a single API review gate:

1. **Checked-in generated public spec.** Generate the normalized public
   `openapi.json` in CI and compare it with the reviewed repository artifact.
   Any route, parameter, schema, response, security, or documentation change
   must appear in the diff and receive normal code review.
2. **Mounted-route/spec parity.** Compare the actual mounted `(method, path)`
   routes with the operations in the public spec. Fail on undocumented routes,
   stale spec operations, duplicate operation IDs, and accidental exposure of
   test-only or internal routes. Prefer an explicit route inventory or runtime
   route metadata over regex-only source discovery.
3. **OpenAPI structural linting.** Enforce project rules for operation IDs,
   tags, summaries, descriptions, request schemas, success/error responses,
   security requirements, path/query parameters, and named schemas. This
   should catch incomplete annotations even when an operation is present.
4. **Breaking-change detection.** Diff the generated spec against the latest
   released baseline and classify removed operations, narrowed schemas,
   newly-required fields, enum changes, response changes, and security changes
   as breaking or review-required. Require an explicit override or release
   note for accepted breaking changes.
5. **Spec-driven contract smoke tests.** Use the OpenAPI document to exercise
   every operation at least for reachability, authentication behavior, input
   validation, unknown-field rejection, expected status families, and response
   schema validation. Start with deterministic seeded fixtures and expand to
   property-based testing only where the endpoint state model permits it.

## Tasks

High — contract wrong or materially incomplete:

- [ ] Register `POST /post/renew-hash-token` in `openapi.rs` — it has an
      utoipa annotation (`auth.rs`) but was never added to the paths list, so
      it is absent from the spec; sibling `/post/renew-timestamp-token` is
      documented (asymmetric omission). Add a coverage test comparing mounted
      routes against the spec so this class of omission cannot recur.
- [x] Document `401` on guarded operations: only 2/65 ops currently declare
      `401` (`authenticate`, `/unauthorized`) while data endpoints sit behind
      `GuardAuth`/`GuardTimestamp` (24 router files). Introduce a reusable
      `Unauthorized` response component and sweep it across `#[utoipa::path]`
      responses (or post-process the generated spec). **Done** — one
      `Unauthorized` `ToResponse` component in `backend/src/openapi_components.rs`,
      registered by the `build.rs` generator from a single `RESPONSE_COMPONENTS`
      list, referenced by 39 operations (2 → 40 declaring a 401: +37 in the
      sweep, +2 when `get-rows`/`get-scroll-bar` were fixed). Each route was
      checked individually: `GuardReadOnlyMode` answers 405;
      `get-rows`/`get-scroll-bar` originally discarded their guard result —
      that gap was fixed (guard propagated, both operations now declare 401)
      and recorded in `bug-get-rows-auth-guard-discarded.md` (done).
      `POST /post/authenticate` lost its specific "Invalid
      password" wording because utoipa cannot attach a description to a `$ref`;
      the shared description covers it. Follow-up: 405 is declared on only one of
      the 20 `GuardReadOnlyMode` routes.
- [x] Remove test-only probes from the public spec: `probe_record`,
      `probe_dupe_group` + `TestRecordProbe`/`DupeGroupMember` schemas are
      registered unconditionally; handlers compile into production and are only
      flag-gated (404 unless test bootstrap). **Done** — `--dump-openapi` now
      serves `openapi_public::public_json()`, which strips `/get/test/*` and
      the two probe schemas; probe contract tests keep the full generated spec.

Medium — bad patterns and type fidelity:

- [x] Tag every operation: 39/65 ops have no tags, the other 26 all carry the
      misnomer `pages`; reference grouping/TOC is effectively random. Suggest
      `auth`, `albums`, `assets`, `config`, `index`, `metadata`, `shares`,
      `serving`, `upload`. **Done** — all 61 public ops now carry exactly one
      tag; distribution: `pages` 22, `albums` 9, `assets` 7, `config` 6,
      `timeline` 6, `index` 5, `auth` 3, `serving` 2, `upload` 1. The
      suggestion list needed two corrections: `metadata` and `shares` were not
      used (their operations belong to `assets` and `albums` respectively),
      and `timeline` — the grid-data group (`prefetch`, `get-data`,
      `get-rows`, `get-scroll-bar`, `get-tags`, `get-export`) — was missing
      from it. Gated by `every_operation_carries_a_known_tag` in
      `backend/src/tests/openapi_contract.rs` (known set, missing tags, and
      `pages`-by-path-shape), with negative self-checks; taxonomy documented
      in `docs/openapi-generator.md`.
- [ ] Add missing schema field descriptions: ~204 `none` cells across ~30 of
      39 schemas (`EditTagsData`, `DeleteList`, `CreateShare`, `Prefetch`, …).
      Existing hand-written `///` comments (`FileEntry`, `coverHash`) are the
      quality bar.
- [ ] Fix nullable/union rendering: `Option<T>` collapses to `any` in tables
      and `{}` in examples (`TestRecordProbe.path`,
      `PrefetchReturn.resolvedShareOpt`, multipart upload `body`). Emit
      explicit nullable schemas so generators keep the type.
- [ ] Fix `FsCompletion` wire casing: serializes `is_default` (snake) while
      every other schema is camelCase — missing
      `#[serde(rename_all = "camelCase")]`. Real API inconsistency, not just
      docs.
- [ ] Fix mangled anchors: 4 operations emit `<h3 id>` attributes containing
      raw newlines/backticks/apostrophes from multi-line utoipa summaries
      (album-index, index-image, probe ops), breaking Parameters/Responses
      in-page links and duplicating description-as-anchor. Keep utoipa
      summaries single-line titles.
- [x] Update `docs/openapi-generator.md`: 4 references to
      `docs/mdbook/src/openapi-reference.md` are stale; `justfile` writes
      `docs/openapi-reference.md`. Anything wiring a CI drift-check against
      the doc path checks a nonexistent file. **Done** — paths corrected, the
      nonexistent `just openapi-docs`/`openapi-docs-check` recipes replaced with
      the real `just docs-openapi`/`just openapi-check`, and the checked-in
      artifact plus the parity gate documented.

Low — consistency and polish:

- [ ] Document remaining `/upload` query params on the utoipa path: only `auto_rename` is annotated; add
      `presigned_album_id_opt` and `on_conflict` (valid values `skip`|`rename`, defaults). Absorbed from
      `openapi-upload-query-params.md` (2026-09-24).
- [ ] Add descriptions to bare enums: `OnConflict` (`skip`/`rename` need
      behavior semantics), `AlbumIndexState`.
- [ ] Decide a naming convention and document it: snake query params
      (`on_conflict`, `auto_rename`) vs camel bodies (`onConflict`) vs mixed
      path styles with verb-doubling (`/get/get-data`). Path changes are
      breaking — documenting the rule is enough for now.
- [ ] Decide static-route policy: `/assets/<file..>` is undocumented while
      the SPA catch-all `/{path}` is documented; pick neither or both.
- [ ] Rename `DataBaseTimestampReturn` (typo-cased legacy name).
- [ ] Optional bloat pass: 65 ops × 9 sample languages ≈ 70% of the 11k-line
      file, including 39 widdershins "backwards compatibility" boilerplate
      hits; consider `widdershins --summary` or trimming sample languages.

## Shared OpenAPI Sanity Work

This is the implementation plan for the first two mechanisms above, not a
separate task. The existing `build.rs` route generator, OpenAPI contract tests
and generated-artifact check should converge on one reusable source/spec
analyzer while keeping runtime checks and endpoint behavior tests separate.

### Step 1: Extract the source analyzer

- Create a workspace crate at `utils/openapi-sanity/` containing the current
  `syn` route discovery and canonical Rocket-to-OpenAPI path translation.
- Make `build.rs`, backend contract tests and the CLI consume the same library;
  remove the current `#[path]` duplication after the consumers are migrated.
- Preserve byte-identical `backend/src/openapi.rs` and
  `backend/openapi.json` output.
- Keep malformed-source diagnostics deterministic and non-panicking.

### Step 2: Make `openapi-check` the unified contract gate

- Add a deterministic CLI semantic check for source/spec consistency:
  missing function-local annotations, path/method mismatches, source/spec
  operation drift, duplicate handler identities and duplicate operation IDs.
- Make `just openapi-check` run the semantic check and then the generated
  artifact diff. `just check` and CI require only this command.
- Keep runtime mounted-route parity in backend integration tests; source
  analysis cannot replace feature-aware runtime inspection.
- Verify each failure mode with an injected mutation and restore it to green.

### Step 3: Align authentication policy with implementation

- Derive observed guard types from handler parameters, distinguishing direct
  guards from deferred `GuardResult` guards.
- Require protected operations to have an approved guard and matching OpenAPI
  `401`/security policy.
- Require deferred guard results to be propagated or inspected.
- Keep public pages and intentional unauthenticated entry points as explicit
  policy exceptions. Do not infer security from subject tags.
- Migrate the existing tag and shared-401 test policy only after the new
  library/CLI produces equivalent diagnostics.

### Step 4: Add parameter and documentation consistency selectively

Only add checks that have actionable source information:

- OpenAPI path parameters match handler parameters;
- optional query parameters match `Option<T>` shapes;
- request-body declarations match `Json<T>` and upload inputs;
- operation IDs remain stable across reviewed spec changes;
- response schemas are referenced and not orphaned.

Do not turn this into a second OpenAPI serializer or a general Rust taint
analyzer. Request-to-panic analysis remains a separate reachability task.

## Progress

- 2026-09-27: Moved the tag taxonomy into the shared policy library (Step 4 of
  the earlier wording — the tag migration promised by Step 3's last bullet, not
  the parameter work the section is otherwise named for). `utils/openapi-sanity`
  gained a `tags` module: `KNOWN_TAGS` is the single declaration of the vocabulary
  and `check_tags` holds a document to it in four rules — an operation with no
  tags, a tag outside the vocabulary, `pages` on a data-API path, and `pages`
  missing from an SPA page path. `SpecOperation` carries `tags` so the rules read
  the committed document the same way the auth rules do, and `openapi-sanity
check` now merges the tag findings into the one report, so `just openapi-check`
  phase 1 covers the taxonomy without compiling the backend. The rules are
  independent, so an untagged page path is reported twice; that is deliberate,
  since it is missing a tag _and_ missing the reserved one. The document-side
  message shape is the auth rules' — `METHOD PATH: …` inside a `file:` label —
  and the verb is upper-cased like every other diagnostic in the crate, where the
  backend's own copy printed the raw JSON key.

  The migration is a move, not a rewrite: the vocabulary, the four rules and the
  `pages` placement logic are the ones `every_operation_carries_a_known_tag`
  enforced, and equivalence was measured before anything was deleted. Three
  mutations of the real router — `timeline` → an unknown `metadata`, the `tag`
  line removed, and `timeline` → `pages` on `GET /get/get-data` — each regenerated
  the artifact and were then run through the old backend test and the new CLI
  rules. All three were caught by both, on the same tree, with the same rule
  (`get /get/get-data: unknown tag \`metadata\``before,`GET /get/get-data:
  unknown tag \`metadata\``after). The same three were re-run after the migration:`cargo test --lib`now reads`check_tags` and fails with the CLI's diagnostic, so
  tag drift is not detectable only through the CLI.

  The backend keeps the document half of it, the Step 3 pattern:
  `the_public_operations_follow_the_shared_tag_taxonomy` runs `check_tags` over
  the generated public spec, with `self_check_detects_tag_drift_in_the_public_spec`
  proving the shared check still notices from where it is called. The six old
  self-checks over hand-written JSON are gone — the equivalent coverage now lives
  in the crate, where the rules are, over an `untagged/` fixture tree that carries
  one instance of each failure mode. The mounted-route parity tests and the
  `Unauthorized`-component tests are untouched: parity is runtime-only, and the
  component tests are document shape the CLI does not duplicate.

  The fixture documents gained the `tags` their annotations already declared —
  the fixtures' sources were tagged all along and only their documents were not,
  so the CLI's report over them would otherwise have changed for an unrelated
  reason. `KNOWN_TAGS` is a hand-written claim in both directions now: besides the
  gate, `the_repository_carries_the_tags_the_taxonomy_names` requires the subjects
  the committed document uses and the subjects the vocabulary names to be the same
  set, so a vocabulary entry with no operation cannot rot in place. One convention
  is not a rule: "exactly one tag per operation" is held by
  `the_repository_gives_every_operation_exactly_one_subject` rather than by a
  check, because the four rules are about an absent, an unknown and a misplaced
  tag and inventing a fifth was not part of a move.

  Gated by 13 tests over the `clean/` and `untagged/` trees (each rule as an exact
  diagnostic plus the whole report, the excluded prefix, the vocabulary itself, and
  two over the committed document), 4 new mutation tests that break one tag in a
  conforming copy and restore it, 1 new CLI test, and 1 new backend test. Every
  rule, the excluded-prefix skip, the page/data classification, the sort and the
  CLI wiring were each neutered in turn and failed a named test; both source files
  were restored byte-identically afterwards. The repository is clean: 61
  operations, 0 findings, and `backend/openapi.json` and every `#[utoipa::path]`
  are unchanged. Step 5 (parameters and documentation) is untouched.

- 2026-09-27: Aligned the authentication policy with the implementation (Step 3).
  `utils/openapi-sanity` gained a `guards` module and an `auth` module, and the CLI
  now runs both checks and prints one merged report. A handler's parameters are
  read for request guards: a bare or referenced `GuardX` is a direct guard Rocket
  runs before the body, a `GuardResult<X>`/`Option<X>` is deferred and only as
  strong as the body makes it — propagated with `?`, returned, matched, or read
  through `is_ok`/`map_err`/`expect`. A wildcard binding (`let _ = auth;`) and a
  binding the body never mentions are both reported against the parameter's line,
  which is the shape `get-rows` and `get-scroll-bar` had before
  `bug-get-rows-auth-guard-discarded.md` fixed them. `KNOWN_GUARDS` is the single
  place a guard type enters the analyzer, and a test reads the backend's
  `FromRequest` implementations to hold it to that claim in both directions.
  Eight guards, not the seven the plan listed: `POST /post/renew-hash-token` is
  guarded by `TimestampGuardModified` — a direct guard accepting an _expired_
  bearer token, since issuing a fresh one is the point — so an operation already
  in the shared-401 policy had no class to name. `GuardClass::ReadOnlyMode` is the
  one guard answering 405, which is why the policy names classes instead of
  counting guards.

  `AUTH_POLICY` is a table with **one entry per documented operation** (61, one per
  line in `utils/openapi-sanity/src/auth.rs`), keyed by `operationId`. Both
  choices are deliberate and documented in the module: listing only the public
  operations would leave the protected set implicit, so deleting a guard from a
  protected handler would leave the policy untouched and the route open — the drift
  the policy exists to catch; and a path key would make every route rename read as
  a new operation (old entry stale, new one unlisted) even though authentication
  did not change. A handler rename does change the id and fails loudly as a stale
  entry instead. Six findings, each a stable `file:line: message`: a protected
  operation with no observed guard, a handler declaring guards the policy does not
  list, a guarded operation documenting no 401, a public operation documenting one,
  an operation in no policy entry, and a policy entry no operation answers to —
  plus the discarded binding from the source scan. A `security` requirement counts
  as documenting the rejection alongside a `401` response, so the rule does not
  have to be revisited when the document gains schemes.

  The repository is clean on the first run: 61 operations, 0 findings. The
  migration of `GUARDED_OPERATIONS` in `backend/src/tests/openapi_contract.rs` is
  partial and deliberate. The two list-based 401 tests are gone, replaced by
  `the_auth_policy_and_the_documented_unauthorized_responses_agree`, which reads
  `AUTH_POLICY` instead of a second list and keeps `cargo test --lib` failing on
  the same drift — verified by removing `(status = 401, response = Unauthorized)`
  from `get_data` and watching it report `GET /get/get-data: the auth policy says
it can answer 401 but it documents none`. The tag policy is _not_ migrated: the
  new gate does not cover tags, and the plan forbids inferring security from them.
  The `Unauthorized`-component checks and the mounted-route parity tests stay in the
  backend regardless — the former is document-shaped, the latter is runtime-only.

  Gated by 24 guard-detection tests, 15 auth tests over a new `unauthored/`
  fixture tree that carries one instance of every auth failure mode (asserted rule
  by rule and as a whole report), 4 mutation tests that break one thing in a
  conforming copy and restore it — a guard removed, a `GuardResult` dropped, a
  public operation unlisted — and the existing 14 CLI tests, one of which now
  covers the merged report with a shared finding printed once. `the_repository_
matches_its_own_policy` runs the policy over the real router, and
  `the_policy_lists_every_documented_operation_and_nothing_else` pins the count.
  `backend/openapi.json` and every `#[utoipa::path]` are unchanged; no inconsistency
  was found in the current source. Step 4 (parameters and documentation) is
  untouched.

- 2026-09-27: Made `just openapi-check` the unified contract gate (Step 2).
  `utils/openapi-sanity` gained a binary, `openapi-sanity check`, and a
  `contract` module; `just openapi-check` is now two phases, the semantic one
  first — annotations vs. `backend/openapi.json` on source, no compiled backend
  needed — then the existing generated-artifact diff, and a failing dependency
  stops the recipe, so either phase fails nonzero with its own diagnostics
  visible. `just check` and CI pick it up unchanged. Seven checks, each
  reported as a stable `file:line: message` and sorted by file, line and
  message: a registered handler with no `#[utoipa::path]`, a route URI and an
  annotation path that disagree after `to_spec_path`, a route attribute and
  annotation verb that disagree, a source operation the document omits, a
  document operation no scanned source declares, a handler identity registered
  twice, and a duplicate `operationId`. Two things the library did not have:
  `Handler::spec_method` (the annotation's verb — utoipa takes it as a bare
  identifier among the annotation's top-level tokens, and it decides which
  operation the handler is registered under) and `HandlerRef::line` (so a
  duplicate registration points at the `routes![]` entry, not at the handler).
  `SCANNED_MODULES` moved from `build.rs` into the crate rather than being
  forked, which surfaced a stale entry: `("fairing", "fairing/mod.rs")` has
  listed a file that has not existed since c6e599a9 flattened
  `router/fairing/*` into `router/auth.rs`, and the build script skipped it
  silently. Removed, with `every_scanned_router_module_exists` added to
  `backend/src/tests/route_scan.rs` so a dead entry cannot come back;
  `openapi.rs` and `openapi.json` regenerate byte-identically. A handler whose
  annotation disagrees with its own route is reported once, locally, and is not
  also reported as document drift — the document inherits the disagreement from
  the annotation, so a second finding would restate the first; the drift fixture
  pins one finding per rule. The exclusion that keeps the test-only probes out
  of the comparison is a `--exclude-prefix` argument rather than a constant in
  the analyzer, because it describes the artifact and not the analysis;
  `just openapi-check` passes `/get/test/`, and running without it is a test.
  The repository is clean: 61 operations, 0 findings. Gated by 20 checks over
  the `clean/` and `drift/` fixture trees in `utils/openapi-sanity/tests/`
  (each rule as an exact diagnostic, plus the whole report), 14 CLI tests, and
  one that runs the gate over the real `backend/src/router` and
  `backend/openapi.json` so it cannot be neutered and stay green. Step 3
  (auth policy) is untouched.

- 2026-09-27: Extracted the source analyzer into `utils/openapi-sanity`
  (Step 1). The crate owns the `routes![]` scan, the per-function
  `#[utoipa::path]` attribution, Rocket route/URI discovery and the
  Rocket-to-OpenAPI path translation; `build.rs` keeps only what needs the
  filesystem — which router files to scan, and writing `openapi.rs` — and
  `openapi_contract.rs` imports the translation instead of defining a second
  copy. `backend/build/route_scan.rs` and its `#[path]`-included test module
  are gone: the parser whose bug made the renewal routes undocumented is now
  covered by 33 black-box tests against the crate's public API. Behaviour
  differs from the text scanner it replaces in four ways, all of them
  deliberate — attribution is per function (a file-level check can credit a
  sibling's annotation), an entry that is not `ident`/`path::ident` becomes a
  finding instead of a silent skip, a syntax error becomes a file/line finding
  instead of a partial scan, and `source` must be a complete file rather than
  a fragment. `backend/src/openapi.rs` and `backend/openapi.json` regenerate
  byte-identically: the per-function check credits exactly the handlers the
  old substring check did for the current router files. `scan_source` parses
  once for both views and exists for the Step 2 CLI, so it is covered by
  tests rather than by `build.rs`.

- 2026-09-25: Tagged every public operation and gated the taxonomy (Medium
  "Tag every operation"). Each `#[utoipa::path]` annotation got one
  `tag = "..."` line (`pages` kept on the 22 SPA routes, `albums` kept on
  `assign_album`); 38 operations were untagged before. Final distribution:
  `pages` 22, `albums` 9, `assets` 7, `config` 6, `timeline` 6, `index` 5,
  `auth` 3, `serving` 2, `upload` 1 (61 total). The plan's suggestion list was
  off: `metadata`/`shares` were folded into `assets`/`albums`, and `timeline`
  (grid data) had to be added; `pages` was already in use. The gate is
  `every_operation_carries_a_known_tag` with the vocabulary in a `KNOWN_TAGS`
  constant and the `pages` expectation derived from data-API path shapes (the
  spec alone cannot say which file annotated an operation — assumption
  documented on `is_data_api_path`). Three mutations (drop the missing-tag
  report, drop the unknown-tag report, drop the data-API/`pages` report) each
  failed their named self-check before reverting. Taxonomy table and
  enforcement noted in `docs/openapi-generator.md`.

- 2026-09-25: Documented the 401 contract. The sweep exposed a real security
  gap rather than only a documentation one: `GET /get/get-rows` and
  `GET /get/get-scroll-bar` discarded `GuardResult<GuardTimestamp>` with
  `let _ = auth;`, so both answered 200 to an unauthenticated caller, and
  `/get/get-scroll-bar` panics on an unknown snapshot id without credentials.
  Recorded as `bug-get-rows-auth-guard-discarded.md` and fixed separately, since
  it is a behavior change rather than documentation: both handlers now
  propagate the guard, both operations declare 401, and two scenarios assert
  401 for missing, malformed and expired tokens. The contract
  tests for 401 are bidirectional: an operation declaring a 401 must be listed in
  `GUARDED_OPERATIONS`, and a listed operation that stops existing fails with a
  stale-entry message instead of a missing-declaration one.
- 2026-09-25: Added negative self-checks so the gates cannot be neutered
  silently. The `routes![]` scanner moved out of `build.rs` into
  `backend/build/route_scan.rs`, shared with `src/tests/route_scan.rs`, because
  a build script cannot be unit-tested in place — its parsing bug is exactly
  what made the renewal routes undocumented. The parity comparison moved into
  pure functions (`undocumented_routes`, `stale_operations`,
  `duplicate_operation_ids`) that `src/tests/openapi_contract.rs` exercises with
  deliberately drifted inputs, and a new test asserts `public_json()` equals the
  committed `backend/openapi.json`, so artifact drift fails `cargo test` and not
  only `just openapi-check`. 13 mutations (neuter each comparison, drop the
  exclusion filter, widen the exclusion, ignore the HTTP method, no-op the path
  normalizer, revert the scanner to line-based parsing, accept non-identifier
  entries, stop stripping comments, end the block at the first bracket, scan
  only the first block, stale artifact) were each verified to fail a named test.
- 2026-09-25: Established hardening mechanisms 1 and 2.
  `backend/openapi.json` is now a committed, pretty-printed, sorted-key
  artifact; `just openapi-check` regenerates and diffs it and is part of
  `just check` (so CI and the `main` pre-commit hook run it).
  `backend/src/tests/openapi_contract.rs` compares Rocket's mounted route table
  with the spec: undocumented mounted routes, documented-but-unmounted
  operations, duplicate `operationId`s, and dead contract exclusions all fail.
  Rocket's `<segment>`/`<segment..>` URI syntax is normalized to OpenAPI
  `{segment}` templates; the test-only probes and the `/assets` file server are
  explicit, reviewed exclusions. Both directions were verified by injecting
  drift: renaming an annotated path fails `every_spec_operation_is_mounted`,
  and un-scanning `router/auth.rs` fails `every_mounted_route_is_documented`.
  Spec operation count went 59 → 61. Remaining: structural linting (3),
  breaking-change detection (4), spec-driven contract smoke tests (5), plus the
  content tasks above. The markdown reference is still not drift-checked
  because `widdershins` is fetched over the network.
- 2026-09-26: Consolidated the proposed `openapi-sanity` plan into this task.
  `openapi-sanity` was an implementation name for the shared source/spec/auth
  work, not an independent feature. The unified gate is `just openapi-check`:
  semantic source/spec checks first, then generated-artifact diffing. Runtime
  parity, endpoint scenarios, Markdown generation and request reachability
  remain separate owners.
- 2026-09-23: Rework-adjacent subset executed by the path-primary cleanup
  sweep (see `.plan/path-primary-cleanup.md`): test-only probes stripped from
  the public spec; `PUT /put/assign_album` given tag `albums`, a single-line
  summary and explicit description (fixes its mangled reference anchors);
  `OnConflict` and `AssignAlbumData.albumId` documented. Reference regenerated.
- 2026-09-23: Dependency for task 1 (`renew-hash-token`): decide
  `path-primary-cleanup` category 7/B3 first — if the route is renamed
  hash→asset token, register the final path instead of documenting the old one
  and then changing it.
- 2026-09-24: Superseding the B3 dependency note: `path-primary-cleanup`
  category 7/B3 was withdrawn (content hash intentionally stays in compressed
  URLs and token claims; route remains `/post/renew-hash-token`). Task 1's
  rename dependency is resolved — it may proceed at any time via the
  `build.rs` `router/auth.rs` route-scan fix.
