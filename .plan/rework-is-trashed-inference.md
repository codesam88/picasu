---
status: idea
type: feature
priority: medium
area: frontend
---

## Context

`isTrashed` on the frontend data model is inferred from alias paths by checking
if they contain `/.trash/`. This is a stopgap because the backend no longer
stores or returns an `is_trashed` boolean.

## Problems

1. **Hardcoded path:** The check uses `/.trash/` as a substring. If the user
   configures a non-default `trash_directory` (e.g., `.picasu_trash`), the
   inference breaks silently — items appear as not-trashed even though they are.

2. **Fragile coupling:** The frontend must know the trash directory name to
   derive trash state. This is a layering violation — the backend owns the
   trash location, and the frontend should consume a boolean, not re-derive it.

3. **Album trash state:** Albums have no aliases. The current code defaults
   `isTrashed` to `false` for albums. If an album is trashed (dir_path under
   `.trash/`), the frontend won't know.

## Options

1. **Restore `is_trashed` as a denormalized field** — compute it from alias
   paths in the backend at index time and on trash/untrash operations. Store
   it in `ObjectSchema`. The frontend reads it from the API as before.

2. **Pass trash_directory to the frontend** — include it in `GET /get/config`
   response. The frontend uses the actual configured value for path checks.
   Fixes problem 1 but not problem 2.

3. **Add a `trashed` field to the API response** — the backend computes and
   returns it per-item in the prefetch/get-data response without storing it.
   No schema change, but the computation happens on every API call.

## Recommendation

Option 1 is the most robust and simplest. Option 3 is a lighter alternative
if avoiding schema changes is preferred.

## user feedback

actually, the trashed status should be globally obvious based on the root path
we're browsing, either we're on the trashed page browsing in .trash, or
elsewhere. Furthermore, as we add more special folders and multi-user support,
we can probably generalize this to support multiple image roots:

- shared / public
- user / private
- trashed

Each of these may trigger different dialogs and options to be available when
browsing them via the frontend, and we can just pass them as a global 'root'
property or similar to the frontend.

In particular:

- shared and trashed are single global folders
- user is a placeholder name with actual backend path selected depending on the
  logged in username
- browsing the trashed folder opens the permanent-delete option
- browsing the shared folder opens the delete(trash) and 'move to private' option
- browsing the user folder opens the 'elete(trash) and 'move to shared' option
- by default, each logged in user can only see shared and their respective own folders

- location of these folders relative to IMAGE_ROOT is configured in the backend.
  the frontend only sees abstracted folder roots and adapts the presented
  interface accordingly.
