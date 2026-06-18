---
status: open
type: feature
priority: medium
area: testing
---

Backend unit tests for pure functions with non-trivial branches, per `docs/test-strategy.md` priority targets:

1. **`prettify_dir_name`** (`dir_album.rs`) — pure string transform; edge cases around separators, casing, unicode
2. **Schema version dispatch** (`ser_de.rs`) — encode/decode round-trips per version; silent corruption if `[0xFF, version]` prefix logic regresses
3. **`Expression` filter predicates** (`generate_filter.rs`) — filters composed at runtime; incorrect predicate logic silently returns wrong results
4. **`compute_timestamp`** (`abstract_data.rs`) — priority logic across EXIF, file, and fallback timestamps
5. **`belongs_to_album` path-prefix branch** (`combined.rs`) — dir-vs-manual discriminator; path-prefix semantics are subtle
