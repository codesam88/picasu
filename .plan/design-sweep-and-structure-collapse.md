---
status: done
type: chore
priority: high
area: backend
---

# Design Sweep and Structure Collapse

Follow-up to the path-primary migration (see `path-primary-asset-execution.md`). There is no legacy data to support: the
policy is clean rebuild only. Three items, executed sequentially — one sub-agent per item, parent reviews and runs the
full gate, then commits before the next item starts.

## Item 1 — Names and docs sweep — done

Update names and documentation (code identifiers, doc comments, `docs/` prose) to reflect the current design: identity
is `asset_id`, the metadata store is `METADATA_TABLE`, content hash is only for dedup and compressed serving. Includes
the frontend `albumHash` route param (carries an asset id) and dead route params with no consumers.

## Item 2 — ser\_de version reset — done

Remove the v1–v6 schema migration arms in `storage/ser_de.rs` (DB migration is not supported), reset `SCHEMA_VERSION`
to 1 with the current structs as the v1 schema, and make unknown/prefixless versions a clear error pointing at rebuild.

## Item 3 — Multi-alias collapse and artifact review — done

Collapse `alias: Vec<FileModify>` on image/video metadata to a single alias (empty = pruned), remove logic that only
existed to choose among multiple aliases, and review for similar artifacts that can now be simplified for the `asset_id`

- `METADATA_TABLE` design. Report on redundancies that should be kept (lean vs fat store) with rationale.

## Wrap-up (final step, after item 3 review) — done

Regenerate derived artifacts against the final state: `just openapi-gen` (local `openapi.json`), `just docs-openapi`
(`docs/openapi-reference.md`), and any other generated docs the repo produces (check `justfile` for docs recipes).
Verify regenerated output is committed where the artifact is tracked (note: `backend/openapi.json` and
`backend/src/openapi.rs` are gitignored; `docs/openapi-reference.md` is tracked). Final full gate: `just check; just
test`.

## Progress notes (newest first)

- **2026-09-23 — wrap-up done.** `just docs-openapi` regenerated `docs/openapi-reference.md` (+932/-77: new
  post/rebuild, get/metadata, dupe-group probe operations; get-data trashed param dropped; FileModify single-alias docs;
  AssignAlbumData/RebuildStats/DupeGroupMember schemas). Local `backend/openapi.json` + `backend/src/openapi.rs`
  regenerated too (gitignored). Final gate green: `just check` 0, `just test` 0 — backend 263 / utils 24 / vitest 65 /
  playwright 34.
- **2026-09-23 — item 3 done (`493b3d3c`).** alias Vec→Option on Image/VideoMetadata (None = pruned); wire shape
  array→object|null swept through zod schemas, 11 frontend files, 6 scenario YAMLs, docs. keep\_view\_alias deleted
  (no-op proven by tests written against the old code before deletion) along with the unused get-data trashed query
  param end-to-end. Artifact review: kept per-alias is\_trashed (follow-up: move to record level — second wire
  change), FileModify.modified/scan\_time (compute\_timestamp sort keys), empty-path flush remove translation, test
  probe 0/1-len vec. SCHEMA\_VERSION stays 1 (clean-rebuild policy covers intermediate-branch DBs). Backend
  lib 255→263. Harness finding: intermediate call-level `response.json.*` assertions are never evaluated (proved
  empirically) — stale asserts pass silently; harness gap logged as follow-up.
- **2026-09-23 — item 2 done (`e31d2f3b`).** ser\_de 1282→305 lines: v1-v6
  structs/From-impls/migrate\_alias/RawRecord and 10 migration tests removed; SCHEMA\_VERSION 7→1; unknown-version and
  prefixless records panic with rebuild instructions (redb 4.2 Value has no error channel — panic is the mechanism);
  index\_v4 startup guard removed from lib.rs; database.md rewrite. Backend lib 262→255.
- **2026-09-23 — item 1 done (`0c9d4ea3`).** albumHash→albumId (route param), extract\_hash\_from\_path→
  extract\_serving\_id\_from\_path, dead hash/subhash params removed, editStore/navigation-prop misnames fixed;
  docs/database.md table inventory + design.md/frontend.md/config.md/scenario-dsl.md prose updated. Found+fixed ViewPage
  overlay arrows writing the discarded hash param (pre-existing nav defect). Backend doc comments updated;
  ClaimsHash/GuardHash/renew-hash-token kept (genuine serving-token machinery, now documented as hash+asset\_id).
