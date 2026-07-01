---
status: backlog
type: chore
priority: low
area: frontend
---

## Frontend Vue architecture — consolidation opportunities

Low-effort survey of the frontend Vue codebase (not tied to a specific diff)
for obvious, cheap-to-fix duplication/simplification. Not urgent; pick up
opportunistically or when touching adjacent code.

### 1. `getIsolationIdByRoute(route)` is dead indirection (lowest risk, do first)

20 files repeat:

```ts
const route = useRoute();
const isolationId = getIsolationIdByRoute(route);
```

Since the route-flattening refactor (`feat/ui-overlays`), this function always
returns `'mainId'` regardless of the route — `'subId'` (the only other value
it used to return) has no consumers left. Fix: inline `'mainId'` at call
sites and delete `getIsolationIdByRoute` from `script/utils/getter.ts`. Also
drop `'subId'` from the `IsolationId` union in `type/types.ts` (keep
`'tempId'` — still used by `GalleryTemp.vue`/`GalleryTempBar.vue`).

### 2. `BaseModal.vue` is under-used

`components/Modal/BaseModal.vue` is a clean, generic modal shell (title,
loading state, close button, actions slot). Currently only used by the
Share-related modals (via `ShareModalBase.vue`). These hand-roll their own
`<v-dialog>` + `<v-card>` + title/close/actions structure instead:

- `AlbumInfoModal.vue`
- `AssignAlbumModal.vue`
- `EditBatchTagsModal.vue`
- `EditTagsModal.vue`
- `ShareDeleteConfirmModal.vue`

Migrating them to `BaseModal` would delete a meaningful chunk of repeated
markup per file. Medium effort (touches 5 files' templates), do as a
dedicated pass rather than incidentally.

### 3. "Edit X" API helpers split inconsistently across two directories

Same category of function (PUT an edit to the backend, patch local store),
two homes with no apparent rule:

- `api/editFlags.ts`, `api/editRating.ts`, `api/editTags.ts`
- `script/utils/editDescription.ts`, `script/utils/createAlbums.ts`,
  `script/utils/quickEditTags.ts`

Lower priority than #1/#2 — fixing means touching every importer, not just
deleting code. Worth doing only if establishing a convention going forward
(e.g. picking `api/` as the one true home and moving the rest).

### Not consolidation targets (checked, no action needed)

- `components/Menu/MenuItem/*.vue` (17 files) — already appropriately small
  and single-purpose.
- Pinia stores (28 files, mostly tiny single-field stores using the
  `useXStore(isolationId)` factory pattern) — idiomatic for this codebase's
  per-isolation-id store design, not duplication worth collapsing.

## Notes

- 2026-07-01: Findings from a low-effort ad hoc scan requested by the user,
  separate from the `feat/ui-overlays`/`feat/album-metadata` diff review.
  Not yet actioned.
