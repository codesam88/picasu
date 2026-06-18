---
status: open
type: decision
priority: high
area: backend
---

Plan for FS/DB consistency reporting across the backend. Not implemented yet — this is a planning/design task.

Problem: `gallery-backend` treats the filesystem and redb as two systems that should stay in sync, but nothing watches for drift. Survey of 10 concrete situations identified in TODO.md with classifications: expected/self-healing, unexpected-but-recoverable, unexpected-and-blocking.

Proposed backend pattern:

- Add `ErrorKind::Inconsistency` for FS/DB drift errors
- Single `log_consistency(severity, context, detail)` helper in `public/error_data.rs`
- Optional `WithWarnings<T>` response envelope for recoverable cases

Proposed frontend pattern:

- Add `warning` to `MessageColor`/messageStore
- Surface warnings from API responses in `AssignAlbumModal.vue` and tag editor

Sequencing: backend infrastructure → wire survey hooks → cross-record stale-alias detection → frontend warnings → document pattern → fix prefetch-expiry bug.

See TODO.md "Robustness: FS/DB consistency reporting" for the full survey table and classification.
