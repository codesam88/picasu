---
status: open
type: feature
priority: medium
area: testing
---

Expand the scenario DSL vocabulary with new verbs needed for regression gap coverage:

1. `share: <album_id>` — create share with capability flags
2. `config: { read_only_mode: true }` — config mutation via API
3. `upload: <file>` — multipart upload
4. `delete_dir: <path>` — filesystem cleanup
5. Share-auth in `call:` (`auth: $share_token`) — authorization via share token

Requires generator.rs changes to emit new fixture calls.
