---
status: open
type: feature
priority: high
area: frontend
---

## UI Overhaul — Replace Navigation v-overlays with Routed Pages + Route Renaming

### Context

The current frontend stacks up to 4 full-screen `v-overlay` components via nested Vue Router child routes:

| Level | Path                                  | Component                                               |
| ----- | ------------------------------------- | ------------------------------------------------------- |
| 1     | `/home`                               | GalleryMain — normal PageTemplate content               |
| 2     | `/home/view/:hash`                    | ViewPage — v-overlay (100% w/h, teleported to `<body>`) |
| 3     | `/home/view/:hash/read`               | HomeIsolated — v-overlay stacked on top of level 2      |
| 4     | `/home/view/:hash/read/view/:subhash` | ViewPage — v-overlay stacked on top of level 3          |

`v-overlay` suits dialogs and transient effects, not full-screen page navigation. The overlays hide the nav bar, require a `'subId'` isolation context for stores so simultaneously-mounted components don't clobber each other, and produce fragile level-counting logic throughout the codebase.

**Goal:** Collapse the 4-level stack to 2. One component fills the content area at a time. Nav bar always visible. `router.back()` for back navigation. Level 3 (HomeIsolated) eliminated — "Enter Album" navigates to `/album/:albumHash` instead.

---

### Phase 0 — Route and file renaming

`/home` is not a distinct view — it's the default landing page. The concept of "home" belongs to the nav item and the `/` redirect. The underlying view is a chronological photo grid → `/timeline`. `/all` (near-duplicate filter) is deleted; "Find in Timeline" points to `/timeline`.

`components/Home/` and `HomeBars/` contain shared gallery infrastructure used by every content page — not home-specific. Renamed to `Gallery`.

**Route-level renaming:**

| Before                                                                                        | After                                  |
| --------------------------------------------------------------------------------------------- | -------------------------------------- |
| `/home`, `baseName: 'home'`                                                                   | `/timeline`, `baseName: 'timeline'`    |
| `/all`, `baseName: 'all'`                                                                     | deleted                                |
| `homePageRoutes`                                                                              | `timelinePageRoutes`                   |
| `'home'` in `BaseName` union (`createRoute.ts`, `pageReturnType.ts`)                          | `'timeline'`; remove `'all'`           |
| `baseTitleMap.home: 'Home'`                                                                   | `baseTitleMap.timeline: 'Timeline'`    |
| `name: 'home'` redirect in `configRoute.ts`, `loginRoute.ts`, `tagsRoute.ts`, `linksRoute.ts` | `name: 'timeline'`                     |
| `albumContentRoute` `getParentPage` → `name: 'home'`                                          | `name: 'timeline'`                     |
| `{ path: '/', redirect: '/home' }`                                                            | `{ path: '/', redirect: '/timeline' }` |

Nav drawer (`Drawer.vue`): keep "Home" label, change `to="/home"` → `to="/"`. "Home" means "go to start page" (`/` redirects to `/timeline`).

`ItemFindInTimeline.vue`: `/all?locate=…` → `/timeline?locate=…`.

`AllPage.vue` deleted. `HomePage.vue` → `TimelinePage.vue` (same filter: `and(not(type:"album"), archived:false, trashed:false)`).

**File/folder renaming (shared gallery infrastructure):**

| Before                                      | After                                   |
| ------------------------------------------- | --------------------------------------- |
| `src/components/Home/`                      | `src/components/Gallery/`               |
| `Home.vue`                                  | `Gallery.vue`                           |
| `HomeMain.vue`                              | `GalleryMain.vue`                       |
| `HomeEmptyCard.vue`                         | `GalleryEmptyCard.vue`                  |
| `HomeScrollBar.vue`                         | `GalleryScrollBar.vue`                  |
| `HomeShare.vue`                             | `GalleryShare.vue`                      |
| `HomeTemp.vue`                              | `GalleryTemp.vue`                       |
| `src/components/NavBar/HomeBars/`           | `src/components/NavBar/GalleryBars/`    |
| `HomeMainBar.vue`                           | `GalleryBar.vue`                        |
| `HomeBarTemplate.vue`                       | `GalleryBarTemplate.vue`                |
| `HomeShareBar.vue`                          | `GalleryShareBar.vue`                   |
| `HomeTempBar.vue`                           | `GalleryTempBar.vue`                    |
| `HomeIsolated.vue`, `HomeIsolatedBar.vue`   | deleted (Phase 7)                       |
| `rerenderStore.homeKey` + `homeIsolatedKey` | `rerenderStore.galleryKey` (single key) |

---

### Phase 1 — Flatten routes to 2 levels

**`src/route/createRoute.ts`**: import `ViewPage` directly; drop level-3 (`read`) and level-4 (`view/:subhash`) children; level-2 uses `ViewPage` with `isolation-id="mainId"` hard-coded.

**`src/route/routes.ts`**: remove `ViewPageMain`, `HomeIsolated`, `ViewPageIsolated` imports; drop `albumReadPage` and `albumReadViewPage` from `albumContentRoute`; remove `ReadPage`/`ReadViewPage` branches from history-chain builders.

### Phase 2 — ViewPage: remove v-overlay

**`src/components/View/ViewPage.vue`**:

- Replace `<v-overlay>` root with `<div class="h-100 w-100 d-flex position-relative bg-background">`
- Delete `overlayVisible` computed; add `keydown` listener for Escape → `router.back()`
- `hash` computed: always `route.params.hash` (drop `subhash` branch)
- Remove `albumFallback` computed and its `v-else-if` block
- Remove `isolationId` prop; hard-code `'mainId'`

### Phase 3 — Gallery.vue: show viewer OR grid

**`src/components/Gallery/Gallery.vue`** (was `Home.vue`):

- `route.meta.level >= 2`: render only `<Transition name="fade"><router-view/></Transition>` filling full height
- `route.meta.level === 1`: render normal toolbar slot + photo grid
- Add fade CSS (opacity 0→1, 150 ms)

### Phase 4 — DisplayAlbum: fix "Enter Album" navigation

**`src/components/View/Display/DisplayAlbum.vue`**:

- Remove `<router-view>` that rendered HomeIsolated
- "Enter Album" → `{ name: 'album', params: { albumHash: props.album.id }, query: route.query }`
- Keep `albumStore.leaveAlbumPath = route.fullPath`

### Phase 5 — Remove level-3/4 conditional logic

| File                                              | Change                                                                                                          |
| ------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| `src/script/utils/getter.ts`                      | `getIsolationIdByRoute`: always `'mainId'`                                                                      |
| `src/script/hook/usePrefetch.ts`                  | Remove `subId`/level-4 `locate` branch                                                                          |
| `src/components/View/Display/Display.vue`         | Drop level-4 from `nextPage`, `previousPage`, `handleKeyDown`                                                   |
| `src/components/View/Display/DisplayMobile.vue`   | `canHandleNav`: remove `level === 4` arm                                                                        |
| `src/components/View/Metadata/MetadataMobile.vue` | `navigateToHash`: remove `level === 4` branch                                                                   |
| `src/components/Gallery/GalleryEmptyCard.vue`     | `searchKey`: remove `subId` ternary; `level === 3` → `baseName === 'album' && level === 1`                      |
| `src/components/Menu/BatchMenu.vue`               | `shouldShowSetAsCover`: `level === 3` → `baseName === 'album' && level === 1`                                   |
| `src/components/Modal/DropZoneModal.vue`          | Remove level-4 early-return; `isLevel3RouteWithHash` → `baseName === 'album' && level === 1 && albumHash param` |
| `src/components/App.vue`                          | Remove `scrollbarStoreInsideAlbum` (`subId`) and its `isDragging` binding                                       |

### Phase 6 — Migrate HomeIsolatedBar features into GalleryBar

**`src/components/NavBar/GalleryBars/GalleryBar.vue`** (was `HomeMainBar.vue`):

- Add "Add photos to album" button (`mdi-image-plus`) gated by `baseName === 'album'`
- Replace `rerenderStore.homeIsolatedKey` increments with `rerenderStore.galleryKey`

### Phase 7 — Delete obsolete files

- `src/components/View/ViewPageMain.vue`
- `src/components/View/ViewPageIsolated.vue`
- `src/components/Gallery/HomeIsolated.vue` (moved during rename, then deleted)
- `src/components/NavBar/GalleryBars/HomeIsolatedBar.vue` (moved during rename, then deleted)
- `src/components/Page/AllPage.vue`

Leave `'subId'` in `IsolationId` union type — remove as a follow-up once runtime usage confirmed gone. `GalleryTemp` (`isolation-id="tempId"`) and genuine modal dialogs are unaffected.

---

### Verification

1. `just frontend-playwright` — all 14 scenarios green
2. Manual: `/` redirects to `/timeline`; nav "Home" item goes to `/`
3. Manual: click a photo → viewer fills content area, nav bar visible, Escape goes back
4. Manual: click album-type photo → "Enter Album" → `/album/:albumHash`, back returns to viewer
5. Manual: add photos via GalleryTempModal in album view → grid refreshes
6. `npx vue-tsc --noEmit` — no type errors
