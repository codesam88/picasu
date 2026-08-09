---
status: open
type: feature
priority: high
area: testing
---

## Notes

Frontend has vitest (pure-logic tests, `environment: 'node'`) and playwright e2e, but
no component/mount testing. This adds a DOM mount layer to vitest and covers the
client-side branch logic that neither suite reaches today.

Overlaps with `pinia-store-tests` (store reducers, no DOM) for the store-level items;
run those first or coordinate so both do not duplicate the same coverage.

### Existing coverage baseline

- Vitest: `lexer.test.ts`, `uploadStore.test.ts` (only `buildUploadUrl`). `vitest.config.ts` runs `environment: 'node'`.
- Playwright: 28 DSL scenarios + 1 hand-written spec. Zero coverage for favorites, archive, videos, share/links, search, batch metadata editing, user settings modal, arrow-key viewer nav, drag-drop upload.

## Scope

### 1. Mount-layer infrastructure

- Add `@vue/test-utils` and `happy-dom` (or `jsdom`) to `frontend/package.json` devDependencies.
- Configure per-file `// @vitest-environment happy-dom` in component tests (keep the default `node` for pure-logic tests).
- Add a shared test harness (`frontend/src/test/` or `frontend/tests/unit/`) that:
  - calls `setActivePinia(createPinia())` and constructs stores via the `isolationId` factory (`useXStore('mainId')`),
  - mounts components with a Vuetify plugin instance (register once, `global.plugins`),
  - provides router mocks (`useRouter`/`useRoute`) and the `provide('windowWidth'|'windowHeight'|'imageContainerRef')` keys that `getInjectValue` depends on,
  - stubs v-treeview / v-combobox / v-dialog where the real widget is heavy.
- Verify `just frontend-test` stays green and `just check` (vue-tsc, eslint, prettier) passes with the new files.

### 2. Store-level tests (no DOM — fits `pinia-store-tests` too)

- `tokenStore`: JWT decode/expiry, `refreshTimestampTokenIfExpired`/`refreshHashTokenIfExpired` with `_renewingTimestamp` dedupe, IndexedDB persistence (needs `fake-indexeddb`).
- `collectionStore`: shift-range multi-select semantics (`addApi`/`deleteApi` + `lastClick`), `leaveEdit`.
- `filterStore.generateFilterJsonString`: search + lexer combination, quote-stripping fallback.
- `uploadStore`: `percentComplete`/`elapsedTime`/`uploadSpeed` getters, `cancelUpload`/abort; extend the existing `buildUploadUrl` test.

### 3. Component tests (DOM layer) — prioritized

1. Menu gating: `BatchMenu`, `SingleMenu`, `AlbumMenu` + the `MenuItem/*` components. Assert trashed-vs-normal action sets (archive/favorite/tags/assign hidden in trash; restore/permanently-delete only in trash), single-selection rules (`ItemAlbumInfo`, `ItemSetAsCover` disabled unless exactly one), `currentFrameStore.video` gating on `ItemRegenerateThumbnailByFrame`, `RotateImage` image-only + non-trashed gating.
2. `AssignAlbumModal`: restore mode prefills `selectedAlbumId` from the item's original album (`restoreDefaultAlbumId`); submit enabled when `isRestore && selectedAlbumId === original` but disabled for a same-album move in normal mode; multi-select restore targets one album; `onConflict` maps `rename` for restore / `skip` for move; album-tree search filter.
3. Modal/Escape orchestration: `modalStore.hasOpenDialog`/`closeOpenDialog`, `PageTemplate` level-1 Escape precedence (close dialog before `collectionStore.leaveEdit()`), `leaveView` behavior.
4. `EditTagsModal`/`EditBatchTagsModal`: flag-item chips (favorite/archived), batch submit over `editModeCollection`, optimistic tag update.
5. `LoginPage`: zod password parse, `redirectionStore` back-vs-replace, empty-password branch.
6. `GalleryBar`: search round-trip through router query + `filterStore`, breadcrumb build capped at 4.
7. `AdvancedConfig`: save blocked when authKey enabled but empty; `ServerFilePicker` pure path helpers (breadcrumb builders, `stripRootPrefix`); `GalleryEmptyCard` per-baseName empty states + upload/scan dialog gating.

### 4. Follow-ups (separate task if taken on)

Browser-flow e2e gaps (favorites, archive, videos, share, search, batch metadata editing,
drag-drop upload) belong in the playwright DSL, not the mount layer.

## Acceptance

- `just frontend-test` green with the new tests.
- `just check` green (vue-tsc, eslint, prettier, plan lint).
- Section 3 items 1–2 (menu gating + AssignAlbumModal) are the minimum viable first step; the rest can land incrementally.

## Progress

(newest first)
