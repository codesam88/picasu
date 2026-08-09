---
status: backlog
type: feature
priority: high
area: backend
---

This needs more elaboration. Multi-user enabling requires extending the
authentication, per-user settings, and some initial thinking about access
control and 'admin' user.

## Context

The album spaces architecture (`shared/`, `trash/`) is designed to extend to
multi-user. Each user gets a private space and a private trash. A shared/private
toggle in the frontend switches the Albums and Trash navigation context.

## Space model

Each space has a directory and a trash target:

| Space  | Directory | Trash target   | Access          |
| ------ | --------- | -------------- | --------------- |
| shared | `shared/` | `trash/`       | all users       |
| admin  | `admin/`  | `admin-trash/` | admin only      |
| bob    | `bob/`    | `bob-trash/`   | bob + permitted |

**Trash routing**: source space determines destination. Items from `shared/`
always go to `trash/`. Items from `admin/` always go to `admin-trash/`.

**Cross-space browsing**: users may view other private spaces if access allows.
Only admin can delete from other users' private spaces, routing to admin trash.

## Frontend toggle

A global Shared/Private switch changes the semantics of Albums and Trash nav:

- Shared mode: `get_albums?space=shared`, trash page shows `trash/` contents
- Private mode: `get_albums?space=<user>`, trash page shows `<user>-trash/`

## Config (future)

Replace flat `shared_directory` / `trash_directory` with a `[[spaces]]` table:

```toml
[[spaces]]
name = "shared"
directory = "shared"
trash = "trash"
access = "all"

[[spaces]]
name = "admin"
directory = "admin"
trash = "admin-trash"
access = "admin"
```

`resolve_space_root(space_name)` looks up the table. Trash routing uses the
source space's `trash` field.

## Dependencies

- Requires: album-spaces (space resolver, `?space=` on `get_albums`)
- Requires: user auth model (who is the current user, what can they access)
- Requires: ACL model for cross-space read/delete permissions

## Tasks

- [ ] Design `[[spaces]]` config table and migration from flat config
- [ ] Add user identity to request context (auth guard)
- [ ] Implement `resolve_space_root` with dynamic space table
- [ ] Add `?space=<user>` support to `get_albums`
- [ ] Implement trash routing by source space
- [ ] Add frontend Shared/Private toggle to nav panel
- [ ] Scope Trash page to current user's trash space
- [ ] Add cross-space browsing with permission checks
- [ ] Admin-only delete from other users' private spaces
