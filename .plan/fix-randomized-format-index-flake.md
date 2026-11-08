---
status: done
type: bug
priority: high
area: backend
---

## Notes

Problem: `scenario_randomized_format_uploads_into_an_album` flakes under
default-parallel nextest with `Index did not complete within 30000 ms
(state=running)` at `backend/src/tests/fixtures/wait.rs:33`. Isolated and
`-j 1` runs pass. Upload always 200; only the index wait times out.

Goal: eliminate the parallel-only flake, prove slow-under-load vs stuck,
drive `just test` green, open scoped PR.

Steps:

1. Instrument `wait_for_album_index` panic with full status JSON.
2. Centralize timeout into one runtime `index_timeout_ms()` helper
   (env-overridable, fallback 90s), replace hardcoded values.
3. Verify parallel group, then `just backend-test`, then `just test`.
4. Open PR from `fix/randomized-format-index-flake`.
