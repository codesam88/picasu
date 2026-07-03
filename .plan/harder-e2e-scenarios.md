---
status: backlog
type: feature
priority: low
area: testing
---

Harder E2E scenarios:

1. **Prefetch snapshot expiry** — blocked on `expire_check` known bug
2. **Video pipeline parity** — needs `ffmpeg`/`ffprobe` in test environment
3. **`renew-hash-token` / `renew-timestamp-token`** — requires expired JWT creation (cryptographic, poor DSL fit)
4. **`regenerate_thumbnail_with_frame`** — multipart + binary file upload (same blocker as `upload` DSL verb)
