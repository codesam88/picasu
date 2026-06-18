---
status: open
type: feature
priority: medium
area: testing
---

Backend integration tests (need tempdir redb):

1. index → dedup → flush → album self-update round-trip
2. dir album path-prefix membership (create album, check file in/out of subtree)
3. schema migration: write a v1-encoded record directly (raw bytes), read it back through `from_bytes`, verify the promoted `dir_path: None` field is present
