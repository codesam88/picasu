---
status: backlog
type: feature
priority: low
area: testing
---

Deferred testing / tooling items:

1. **Playwright E2E** — needs running backend; save for CI
2. **cargo bench / criterion** — indexing pipeline regression benchmarks (file hashing, EXIF extraction, thumbnail generation, redb write throughput, serving latency under load)
3. **Admin status API** — expose queue depth, active indexing jobs, recent errors, per-job progress as a JSON endpoint
4. **Frontend progress UI** — poll existing `/get/import/folder/status` pattern (or future SSE stream) to show active indexing progress to authenticated users
5. **Structured logging** — investigate `tracing` + `tracing-subscriber` as replacement for `env_logger` to support spans, JSON output, and `tracing-journald` for systemd/journald integration
