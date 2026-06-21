---
status: done
type: chore
priority: medium
area: testing
---

Move test fixtures from `gallery-backend/src/tests/api.rs` to `xtask/src/fixtures/`:

- Add `rocket`, `serde_json`, `image` deps to xtask Cargo.toml
- Update generated test preamble to `use xtask::fixtures::*`

Closed: approach diverged. `jpeg.rs` was deleted and image generation was
moved into `xtask::test_image::generate_batch` instead. Remaining fixtures
(auth, discover, wait) stayed in the backend — no benefit to moving them.
