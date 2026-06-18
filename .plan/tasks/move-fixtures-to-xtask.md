---
status: open
type: chore
priority: medium
area: testing
---

Move test fixtures from `gallery-backend/src/tests/api.rs` to `xtask/src/fixtures/`:

- Add `rocket`, `serde_json`, `image` deps to xtask Cargo.toml
- Update generated test preamble to `use xtask::fixtures::*`
