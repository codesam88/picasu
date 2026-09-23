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

## Tasks

High — contract wrong or materially incomplete:

- [ ] Register `POST /post/renew-hash-token` in `openapi.rs` — it has an
      utoipa annotation (`auth.rs`) but was never added to the paths list, so
      it is absent from the spec; sibling `/post/renew-timestamp-token` is
      documented (asymmetric omission). Add a coverage test comparing mounted
      routes against the spec so this class of omission cannot recur.
- [ ] Document `401` on guarded operations: only 2/65 ops currently declare
      `401` (`authenticate`, `/unauthorized`) while data endpoints sit behind
      `GuardAuth`/`GuardTimestamp` (24 router files). Introduce a reusable
      `Unauthorized` response component and sweep it across `#[utoipa::path]`
      responses (or post-process the generated spec).
- [ ] Remove test-only probes from the public spec: `probe_record`,
      `probe_dupe_group` + `TestRecordProbe`/`DupeGroupMember` schemas are
      registered unconditionally; handlers compile into production and are only
      flag-gated (404 unless test bootstrap). Gate registration behind
      `cfg(test)` or exclude from `--dump-openapi`.

Medium — bad patterns and type fidelity:

- [ ] Tag every operation: 39/65 ops have no tags, the other 26 all carry the
      misnomer `pages`; reference grouping/TOC is effectively random. Suggest
      `auth`, `albums`, `assets`, `config`, `index`, `metadata`, `shares`,
      `serving`, `upload`.
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
- [ ] Update `docs/openapi-generator.md`: 4 references to
      `docs/mdbook/src/openapi-reference.md` are stale; `justfile` writes
      `docs/openapi-reference.md`. Anything wiring a CI drift-check against
      the doc path checks a nonexistent file.

Low — consistency and polish:

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

## Progress
