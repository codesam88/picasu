---
status: idea
type: feature
priority: medium
area: backend
---

# Per-dimension version counters for prefetch cache invalidation

## Problem

`VERSION_COUNT_TIMESTAMP` is a single global `AtomicI64` included in every
prefetch cache key (`build_cache_key`). Any mutation — tag edit, flag change,
album assignment, watcher-detected file change, manual scan, upload — advances
this counter (some synchronously, others via the async
`FlushTreeTask → UpdateTreeTask → UpdateExpireTask → swap()` pipeline). A
single bump invalidates **all** cached query results, even those that query
data dimensions the mutation didn't touch.

On a server with many users, one person editing tags causes every other user's
prefetch to miss — every album view, every extension search, every fulltext
query. This is unnecessary work that could be avoided with finer-grained
invalidation.

## Approach

Replace the single global counter in the cache key with **per-dimension**
atomic counters. A cache key only hashes the counters relevant to the filter
expression. A mutation only bumps the counters for the dimension it affects.

Queries over unrelated dimensions keep hitting the cache (both DashMap and
redb), unaffected by the mutation.

## Proposed solution

### New counters (in `backend/src/storage/db.rs`)

| Counter           | Tracks                                          | Bumped by                                                    |
| ----------------- | ----------------------------------------------- | ------------------------------------------------------------ |
| `TAG_VERSION`     | Tag values                                      | `edit_tag`                                                   |
| `FLAG_VERSION`    | Trashed/Archived/Favorite                       | `edit_flags`                                                 |
| `ALBUM_VERSION`   | Album membership, path aliases, album metadata  | `assign_album`, `edit_album` (title, cover), sub-album moves |
| `CONTENT_VERSION` | Item inventory: new, deleted, or replaced files | Watcher create/modify/remove, manual scan, upload            |

`VERSION_COUNT_TIMESTAMP` stays — it still serves redb table naming and expire
management. It is simply removed from `build_cache_key`.

### Cache key derivation

`build_cache_key` walks the filter expression and collects the relevant
counters:

| Expression                                          | Needs counters                                  |
| --------------------------------------------------- | ----------------------------------------------- |
| `Tag(..)`                                           | `TAG_VERSION`                                   |
| `Trashed`, `Favorite`, `Archived`                   | `FLAG_VERSION`                                  |
| `Album(..)`, `Path(..)`, `RootAlbum`, `ParentAlbum` | `ALBUM_VERSION`                                 |
| `Ext(..)`, `Model(..)`, `Make(..)`, `ExtType(..)`   | `CONTENT_VERSION`                               |
| `Any(..)`                                           | `TAG_VERSION + ALBUM_VERSION + CONTENT_VERSION` |
| No expression                                       | all four                                        |

### Mutation route bumps (sync, after tree rebuild)

Each user-facing mutation bumps its dimension counter synchronously (same spot
where `assign_album.rs` currently bumps `VERSION_COUNT_TIMESTAMP`):

- `edit_tag` → `TAG_VERSION.fetch_add(1)`
- `edit_flags` → `FLAG_VERSION.fetch_add(1)`
- `assign_album`, `set_album_cover`, `set_album_title` → `ALBUM_VERSION.fetch_add(1)`
- `edit_rating`, `edit_description`, `edit_share`, `rotate_image`, `regenerate_thumbnail` → no bump (don't affect any filter)

### Scan, upload, and watcher — the common code path

"Manual scan" (user-initiated re-index), "upload image" (new file via HTTP),
and "watcher create/modify" all converge into
`crate::workflow::index_image()` (or more precisely, the same re-indexing
pipeline: `index_image` → `FlushTreeTask` → `UpdateTreeTask` →
`UpdateExpireTask`). Watcher removals go through
`submit_removal_to_watcher` → `FlushTreeTask` → `UpdateTreeTask`.

The current system leaves a stale-window race: after the tree rebuild but
before the async `VERSION_COUNT_TIMESTAMP` swap, a prefetch returns cached
data from before the change. The `assign_album` sync bump was added to close
this window for that specific route, but scan/upload/watcher never got one.

With dimension counters, this window gets wider for unrelated queries (DashMap
entries persist across global-timestamp-only bumps). So these paths need a
sync bump too.

The challenge is finding the right single point to bump. For watcher removals,
`submit_removal_to_watcher` already awaits `UpdateTreeTask`. For
scan/upload/watcher creates/modifies, the pipeline is spawned as fire-and-forget.

**Two options:**

1. **Bump at the caller** — in `start_watcher.rs`, `scan.rs`, `upload.rs`, bump
   all four counters at the point where we know the change is committed. For
   watcher removals this is straightforward (after the awaits). For
   creates/modifies it requires either blocking on the pipeline or bumping
   eagerly (over-invalidation when nothing changed, but acceptable for
   infrequent events).

2. **Bump inside the pipeline** — push the bump into `UpdateTreeTask` (the
   final synchronous step of the pipeline that runs on `INDEX_RUNTIME`). This
   catches every path (scan, upload, watcher, mutation routes) in one place.
   But it conflates dimension concerns — `UpdateTreeTask` doesn't know what
   changed. It would need to bump all four counters, the same broad
   invalidation as the current system for these events.

   A variant: have `UpdateTreeTask` accept a `changed_dimensions` bitmask so
   each caller can specify what changed.

Option 1 is simpler for a first pass and keeps the bump logic colocated with
the mutation. Option 2 is more maintainable long-term but needs a pipeline
refactor.

### Test implications

The `assign_album_cache_invalidation_with_barrier` test and
`EXPIRE_VERSION_BARRIER` were already removed from HEAD before this plan. No
existing test directly asserts `VERSION_COUNT_TIMESTAMP` behavior in the cache
key. New tests needed:

- Unit test for `build_cache_key`: same expression + different dimension
  counters → different keys; different expressions + same counters → different
  keys; same expression + same counters → same key.
- Integration test: mutate tags, verify an `Ext(..)` query still hits cache
  afterwards.

### What stays the same

- `VERSION_COUNT_TIMESTAMP` continues to advance via `UpdateExpireTask` (the
  async `swap()`). It is still the redb table name and the expire scheduling
  mechanism.
- The DashMap ↔ redb two-layer cache is unchanged.
- `ExpireCheckTask` continues to delete old redb tables without modification.

### Open questions

1. **Scan/upload bump point**: caller-side or pipeline-side? See above.
2. **`Any(..)` searches album path**: When a file changes albums, the file's
   path alias changes. `Any` searches path aliases. Does `edit_album` + `Any(..)`
   need both `ALBUM_VERSION` and `CONTENT_VERSION`, or just `ALBUM_VERSION`?
   Currently `Any` depends on `TAG_VERSION + ALBUM_VERSION + CONTENT_VERSION` in
   this proposal — `CONTENT_VERSION` covers content identity changes (new hash),
   but album moves don't change content identity. This is a refinement for later.
