---
status: idea
type: feature
priority: medium
area: backend
---

Wrap image repo file operations and DB updates into a single transaction. Similar to journaling filesystems: a marker (mutex? file log) records the planned transaction, performs the DB update, then file update, then releases the transaction lock. On crash before file modification, detect unfinished transaction and either rollback or complete to ensure consistency. Must work on any local filesystem.

See `docs/design.md` "Keep it robust, low footprint, modular" — consistency via common transaction with journal.
