---
status: in-progress
type: chore
priority: high
area: backend
---

# Design Sweep and Structure Collapse

Follow-up to the path-primary migration (see `path-primary-asset-execution.md`). There is no legacy data to
support: the policy is clean rebuild only. Three items, executed sequentially — one sub-agent per item, parent
reviews and runs the full gate, then commits before the next item starts.

## Item 1 — Names and docs sweep — in progress

Update names and documentation (code identifiers, doc comments, `docs/` prose) to reflect the current design:
identity is `asset_id`, the metadata store is `METADATA_TABLE`, content hash is only for dedup and compressed
serving. Includes the frontend `albumHash` route param (carries an asset id) and dead route params with no
consumers.

## Item 2 — ser_de version reset — pending

Remove the v1–v6 schema migration arms in `storage/ser_de.rs` (DB migration is not supported), reset
`SCHEMA_VERSION` to 1 with the current structs as the v1 schema, and make unknown/prefixless versions a clear
error pointing at rebuild.

## Item 3 — Multi-alias collapse and artifact review — pending

Collapse `alias: Vec<FileModify>` on image/video metadata to a single alias (empty = pruned), remove logic that
only existed to choose among multiple aliases, and review for similar artifacts that can now be simplified for
the `asset_id` + `METADATA_TABLE` design. Report on redundancies that should be kept (lean vs fat store) with
rationale.

## Wrap-up (final step, after item 3 review) — pending

Regenerate derived artifacts against the final state: `just openapi-gen` (local `openapi.json`), `just
docs-openapi` (`docs/openapi-reference.md`), and any other generated docs the repo produces (check `justfile`
for docs recipes). Verify regenerated output is committed where the artifact is tracked (note:
`backend/openapi.json` and `backend/src/openapi.rs` are gitignored; `docs/openapi-reference.md` is tracked).
Final full gate: `just check; just test`.
