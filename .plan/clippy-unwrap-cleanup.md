---
status: backlog
type: chore
priority: medium
area: backend
---

Retire ~140 `clippy::unwrap_used` call sites across the backend. Currently set to `warn` in `Cargo.toml` and excluded from `-D warnings` in the precommit hook.

Most call sites worth fixing first are the ones touching redb lookups and filesystem state — these overlap with the FS/DB consistency reporting effort (see `fs-db-consistency-reporting.md`). Convert `.unwrap()` calls to proper `Option`/`Result` handling or `expect()` with context.

See `docs/test-strategy.md` § "Tooling gaps."
