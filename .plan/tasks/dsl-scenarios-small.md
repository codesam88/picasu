---
status: open
type: feature
priority: medium
area: testing
---

Small DSL vocabulary additions needed for 4 scenario gaps:

1. **Stale `DIR_ALBUM_CACHE`** — delete album directory externally, then `assign_album` → 400 with distinguishable message. Needs `given: remove_dir` (currently only `remove_file`).
2. **`POST /upload` with known hash, different album** — last-write-wins on `album` field. Needs multipart form support or `upload:` fixture verb.
3. **`GET /object/compressed` with wrong `GuardHash`** — valid share + wrong-hash token → 401. Needs bearer-token assertion support.
4. **Share capability flags** — `show_metadata: false` strips metadata, `show_download: false` suppresses download token, `show_upload: false` gates upload. Needs share-creation fixture and share-auth in `call:`.

Each requires a generator.rs change before the YAML scenario can be written.
