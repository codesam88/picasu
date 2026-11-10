# inotify filesystem watcher: rescan as the consistency backstop

Distilled from Syncthing (watcher + periodic rescan) and Watchman
(query consistency via ticks/cookies, audit). Applies to this repo's
watcher in `backend/src/tasks/batcher/start_watcher.rs` (`notify`
8.2.1 over inotify/kqueue/FSEvents): the watcher is the fast path,
the rescan is the bound on convergence.

## Rules

1. Watcher plus periodic rescan, never watcher alone. Proving no event
   was missed is proving a negative (Syncthing keeps hourly full scans
   with the watcher on; overflow and platform deficiencies are the
   reasons). The rescan interval is the documented upper bound on
   convergence — "eventual" without a bound is not a contract.
2. Debounce and order submissions: accumulate change events briefly,
   scan deletes last (Syncthing delays deletes; deletes scanned after
   creates avoid resurrecting rows). A change observed mid-pipeline
   must supersede, never apply over, newer state (generation counters
   or equivalent; stale items are dropped, never applied).
3. Reads never observe partial writes. Derived artifacts are written
   atomically (temp file plus rename, or immutable content-addressed
   paths) or readers are fenced. A torn read is corruption, not
   staleness, and retried reads do not heal it if the bytes on disk
   are already mixed.
4. Audit the index against the source. A crawl-vs-query comparison
   (`watchmanctl audit` pattern) is the operational check that the
   convergence bound holds; simulator traces and unit properties cannot
   see missed submissions.
5. Presentation distinguishes no-data from stale-data. Skeleton only on
   first load; on refetch keep old content and mark it updating (dim,
   never blank). Errors and known-deleted items get explicit states
   (message plus retry, tombstone) — a skeleton that waits on a failed
   request waits forever.
6. Stale values must look stale where acting on them matters. Dim or
   disable consequential actions on unconfirmed data; carry the version
   the action was computed from so the server can reject stale
   submissions.

## Mapping to this repo

- Rules 1–2: watcher debounce plus `GENERATIONS` supersede
  (`backend/src/tasks/queue/intake.rs`), `is_stale` dequeue guards,
  `SweepStalePaths` as the recrawl; missing: overflow story, stated
  convergence bound, deletes-last ordering, generation bumps on all
  mutating paths.
- Rule 3: `NoTornRead` is modeled, `torn_thumbnail` guards it.
  Thumbnails already write atomically (temp sibling plus rename,
  `backend/src/process/thumbnail.rs:write_atomically`); `save_with_format`
  writes to the temp path, never the destination. Unverified for the
  video transcode and sidecar/albuminfo writes.
- Rule 4: no crawl-vs-rows audit tool exists.
- Rules 5–6: grid placeholders and scan-status surfacing are partial;
  per-gap presentation states are unrecorded.
