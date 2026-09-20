---
status: idea
type: feature
priority: medium
area: backend
---

# Database Engine Evaluation

## Question

The path-primary asset design needs an authoritative embedded database for
assets, canonical paths, albums, hashes, query state, and schema generations.
The current implementation uses Redb 4.1 for the main index and disposable
snapshot/cache stores.

Determine whether Redb remains appropriate or whether SQLite, another embedded
engine, or a smaller hybrid is a better fit.

## Unverified Assumptions

The following must be verified rather than assumed:

- which indexes need to be materialized;
- whether filtering remains snapshot-based or moves into database queries;
- whether index intersection belongs in application code or an engine query
  planner;
- what transaction/recovery guarantees are needed around filesystem moves;
- whether a journal is required, or whether rebuildable state plus pending
  operation records is sufficient;
- whether Redb's read/write behavior is adequate at the expected scale.

## Candidates

### Redb

Prototype ordered tables for canonical paths, asset IDs, composite hash
membership, and path-prefix scans. Measure multiple-table transactions,
reader behavior during writes, database growth, reopen time, and large
duplicate groups. Verify the documented schema/table-generation and recovery
behavior instead of assuming the application must implement all of it.

### SQLite

Prototype unique path constraints, composite hash membership, album/date/tag
indexes, combined filters, WAL behavior, migrations, and rebuild tooling.

### Other Engines

Only evaluate RocksDB/other LSM stores or a server database if measurements
show a concrete requirement. Avoid two authoritative engines without a proven
need.

## Prototype Schema

Both candidates should implement the same minimum model:

```text
assets(asset_id, canonical_path, type, blake_hash, metadata, state)
albums(album_id, canonical_path, metadata)
asset_hash(asset_id, blake_hash)
```

Operations to benchmark:

- index/update one path;
- list assets under an album directory;
- locate by asset ID and path;
- find all assets for a hash;
- combine album/tag/date/state filters;
- move and delete one asset;
- rebuild indexes from filesystem records;
- activate a rebuilt database generation.

## Filesystem Recovery Question

Verify behavior for process termination during:

- filesystem move before database commit;
- database commit before filesystem move;
- sidecar move failure;
- delete/trash transitions;
- rebuild generation activation.

Then decide whether the selected design needs a durable journal, a pending
operation record, startup reconciliation, or only filesystem-driven rebuild.

## Benchmark Plan

Use synthetic and sampled metadata at 1M and 10M assets. Measure:

- point lookup by asset ID and path;
- album path-prefix listing;
- combined filters;
- duplicate-group lookup;
- batched indexing, move, and delete throughput;
- reader latency during writes;
- rebuild duration;
- database size and reopen time;
- crash/reopen/reconciliation behavior.

Report p50/p95 latency, throughput, peak memory, write amplification, storage
size, and recovery time. Do not infer production limits from Big-O alone.

## Decision Output

Record the selected engine/version, authoritative tables, cache/snapshot
responsibilities, schema-generation procedure, filesystem recovery behavior,
and measured scale limits.
