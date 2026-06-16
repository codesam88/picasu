# Frontend Architecture Reference

## Stack Overview

The frontend is a **Vue 3 single-page application** written in **TypeScript**, built with **Vite**.

| Layer | Technology | Role |
|-------|-----------|------|
| Language | TypeScript | Type-safe JS; all `.ts` and `.vue` files |
| UI framework | Vue 3 (Composition API) | Reactive component system |
| Component library | Vuetify 3 | Pre-built UI widgets (`v-overlay`, `v-btn`, etc.) |
| State management | Pinia | Global reactive stores shared across components |
| Routing | Vue Router 4 | URL ↔ component mapping, nested routes |
| HTTP | Axios | API calls to the Rust backend |
| Build | Vite | Dev server + production bundler |
| Type checking | vue-tsc | TypeScript checking for `.vue` files |

---

## How Vue Works (fundamentals)

### Single-File Components (.vue)

Every UI piece is a `.vue` file with three sections:

```
<template>   ← HTML-like markup that Vue renders to the DOM
<script>     ← TypeScript logic (Composition API)
<style>      ← Scoped CSS
```

The `<template>` is **reactive**: any variable from `<script>` that changes causes the DOM to update automatically. You never call render() manually.

### Reactive Primitives

Three building blocks from Vue:

- **`ref(value)`** — a box holding one value. Read it with `.value`; write to `.value` to trigger a re-render.
- **`computed(() => expr)`** — a cached derived value. Recomputes automatically when its dependencies change. Read-only unless you define a getter+setter.
- **`watch(source, fn)`** — runs a side-effect function whenever `source` changes.

Example mental model:
```
const count = ref(0)           // box containing 0
const doubled = computed(...)  // always count.value * 2, auto-updated
watch(count, () => { ... })    // runs whenever count.value changes
```

### Props and Events

Components communicate downward via **props** (parent passes data to child) and upward via **emits** (child signals events to parent). In this codebase, Pinia stores are used heavily instead of prop-drilling for shared state.

### `onBeforeMount` / `onMounted`

Lifecycle hooks that run code at specific moments:
- `onBeforeMount` — just before the component's DOM is created. Used here to set initial data (e.g. reading route params to set `basicString`).
- `onMounted` — after the DOM exists. Used here to start prefetch/scroll listeners.

---

## How Vue Router Works

Vue Router maps URLs to components. This app uses **nested routes**: a parent component stays on screen while a child component renders inside it.

### `<router-view>`

A `<router-view>` tag in a component's template is a **slot** where the router inserts the matched child component. It is invisible until the URL reaches that depth.

```
URL: /albums                  → router-view in App.vue shows AlbumsPage
URL: /albums/view/abc         → router-view in App.vue shows AlbumsPage
                                 router-view in Home.vue shows ViewPageMain
URL: /albums/view/abc/read    → all of the above, plus
                                 router-view in DisplayAlbum.vue shows HomeIsolated
```

Each level deeper requires one more `<router-view>` to exist somewhere in the component tree.

### `useRoute()` / `useRouter()`

- `useRoute()` — returns the current route object: `.params`, `.query`, `.meta`, `.name`. Reactive; updates when the URL changes.
- `useRouter()` — returns the router instance for programmatic navigation: `router.push(...)`, `router.back()`.

### Route Params

- `:hash` — the ID of the item opened at level 2/3 (the parent album or photo)
- `:subhash` — the ID of the item opened at level 4 (a photo inside an album)

---

## How Pinia Stores Work

A **store** is a globally shared reactive object. Any component can read or write it; changes propagate to all components that use it.

```
defineStore('myStore', {
  state: () => ({ count: 0 }),         // reactive data
  actions: { increment() { ... } }     // methods that mutate state
})
```

In this codebase every store is **keyed by isolationId** so the main grid and the album grid each get their own independent instance:

```
useDataStore('mainId')  // one store instance for the level-1 page
useDataStore('subId')   // separate store instance for the level-3 album
```

Same store name + same ID = same singleton. Different ID = different instance.

---

## How Vuetify's `v-overlay` Works

`v-overlay` is a full-screen layer that renders **outside the normal component tree** (Vuetify teleports it to `<body>`). It stacks visually using z-index.

This is how levels 2, 3, and 4 layer on top of each other: each level's component contains a `v-overlay` that covers 100% of the screen. The deepest active overlay wins visually. Closing the overlay (ESC or back gesture) calls `router.back()`, which pops the URL back one level and Vue Router unmounts the overlay component.

Because overlays teleport to `<body>`, a component can be positioned anywhere in the component tree yet still cover the whole screen.

---

## Route Levels

Every page section (home, albums, all, etc.) shares the same 4-level nested route structure, parameterized by `baseName`:

| Level | URL pattern | Route name | Component |
|-------|-------------|------------|-----------|
| 1 | `/{baseName}` | `{baseName}` | Page component (e.g. `AlbumsPage`) |
| 2 | `/{baseName}/view/:hash` | `{baseName}ViewPage` | `ViewPageMain` |
| 3 | `/{baseName}/view/:hash/read` | `{baseName}ReadPage` | `HomeIsolated` |
| 4 | `/{baseName}/view/:hash/read/view/:subhash` | `{baseName}ReadViewPage` | `ViewPageIsolated` |

**baseName values:** `home`, `all`, `favorite`, `archived`, `trashed`, `albums`, `videos`, `tags`, `share`

Level 1 is always visible. Levels 2–4 each render as a full-screen `v-overlay` on top of the previous level. The user navigates deeper by clicking items; `router.back()` closes the topmost overlay.

---

## Component Tree

```
App.vue
└── <router-view>  ← renders level 1
    └── {baseName}Page  (e.g. AlbumsPage)
        └── PageTemplate
            └── HomeMain  (isolation-id="mainId")
                └── Home.vue  ← the core grid
                    └── <router-view>  ← renders level 2
                        └── ViewPageMain  (isolation-id="mainId")
                            └── ViewPage.vue  [v-overlay]
                                ├── [image/video] → media viewer
                                └── [album] → DisplayAlbum.vue
                                    └── <router-view>  ← renders level 3
                                        └── HomeIsolated  [v-overlay]
                                            └── Home.vue  (isolation-id="subId")
                                                └── <router-view>  ← renders level 4
                                                    └── ViewPageIsolated  (isolation-id="subId")
                                                        └── ViewPage.vue  [v-overlay]
                                                            ├── [image/video] → media viewer
                                                            └── [album] → DisplayAlbum.vue
                                                                └── <router-view>  (no level 5; empty)
```

**Important:** `HomeIsolated` only reaches the screen if `DisplayAlbum.vue` is rendered first (to provide its `<router-view>`). `DisplayAlbum.vue` only renders if `ViewPage.vue` at level 2 can resolve the hash — either from `dataStore` or from `albumStore` (fallback).

---

## Key Components

### Home.vue
The reusable infinite-scroll photo grid. Used at both level 1 (main page) and level 3 (inside an album).

- **Props:** `basicString` (filter query), `isolation-id` ("mainId" or "subId"), `searchString`
- **Contains:** a `<router-view>` that the next overlay level renders into
- **Key behavior:** calls `usePrefetch` on mount to fetch rows from the backend

### ViewPage.vue
Full-screen v-overlay that shows a single item (photo, video, or album detail).

- Receives `isolation-id` from its parent (`ViewPageMain` passes "mainId"; `ViewPageIsolated` passes "subId")
- Looks up `:hash` (level 2) or `:subhash` (level 4) in `dataStore(isolationId)`
- If item type is `album` → renders `DisplayAlbum.vue`; if image/video → renders media viewer
- **Fallback:** when `isolationId === 'mainId'` and the hash is not in `dataStore`, checks `albumStore` so sub-albums can be shown even though they are absent from the main page grid

### DisplayAlbum.vue
Shown inside `ViewPage.vue` when the current item is an album.

- Shows cover image and metadata card
- Shows an **"Enter Album"** button only at level 2 (`route.meta.level === 2`), which navigates to level 3
- Contains a `<router-view>` — this is how `HomeIsolated` (level 3) renders on top

### HomeIsolated.vue
Level 3 component: a full-screen v-overlay wrapping `Home.vue` with a filter scoped to one album.

- `basicString` = `and(trashed:false, or(album:"<id>", parent_album:"<id>"))` — includes both direct photos and child albums
- Reads the album metadata from `dataStore('mainId')` first; falls back to `albumStore` for sub-albums not present in the main page grid
- Displays the album title bar (`HomeIsolatedBar`)

---

## Data Stores (Pinia, keyed by isolationId)

| Store | Key | Contents |
|-------|-----|----------|
| `dataStore` | `mainId` | Grid items (photos/videos/albums) matching the level-1 page filter |
| `dataStore` | `subId` | Grid items matching the current album filter (level 3) |
| `albumStore` | `mainId` | **All** albums from `/get/get-albums` — keyed by `albumId`. Shared across all components. |
| `prefetchStore` | `mainId`/`subId` | Scroll/prefetch state for each grid |
| `collectionStore` | `mainId`/`subId` | Selected items (edit mode) |

`albumStore('mainId')` is fetched once per session (by `usePrefetch`) and contains every album including sub-albums, regardless of the current page filter. `dataStore` only contains what the current page's `basicString` filter returns.

---

## Isolation IDs

Components that exist in both the main grid (level 1) and the album grid (level 3) use an `isolationId` to keep their state separate:

- `"mainId"` — top-level page (levels 1 and 2)
- `"subId"` — inside an open album (levels 3 and 4)

Stores instantiated with the same ID are shared (Pinia singletons by ID). Passing a different ID creates a separate store instance. This is why the album grid's scroll position, selection state, and loaded items don't interfere with the main grid's.

---

## Navigation Rules

| From level | Click on | Navigates to | Route name |
|------------|----------|--------------|------------|
| 1 (grid) | photo/video | level 2 viewer | `{baseName}ViewPage` |
| 1 (grid) | album | level 3 album grid | `{baseName}ReadPage` |
| 2 (DisplayAlbum) | "Enter Album" button | level 3 album grid | `{baseName}ReadPage` |
| 3 (album grid) | photo/video | level 4 viewer | `{baseName}ReadViewPage` |
| 3 (album grid) | sub-album | level 3 album grid for sub-album | `{baseName}ReadPage` |
| any | back gesture / ESC | one level up | `router.back()` |

Clicking an album at level 1 or 3 goes directly to `{baseName}ReadPage` (level 3). Because level 2 sits between level 1 and 3 in the URL, `ViewPage.vue` at level 2 still mounts — it must render `DisplayAlbum.vue` to provide the `<router-view>` for level 3 (`HomeIsolated`).

---

## Filter System

Filters are string expressions parsed by the Chevrotain lexer (`lexer.ts`). Each page sets a `basicString` that determines what `dataStore` contains:

| Page | basicString |
|------|-------------|
| HomePage | `and(archived:false, trashed:false)` |
| AlbumsPage | `and(type:"album", trashed:false, root_album:true)` |
| AllPage | `and(archived:false, trashed:false)` |
| FavoritePage | `and(favorite:true, trashed:false)` |
| HomeIsolated | `and(trashed:false, or(album:"<id>", parent_album:"<id>"))` |

`parent_album:"<id>"` matches album objects whose directory parent is the given album. It is always false for images/videos.

---

## API Calls (frontend → backend)

| Endpoint | Used by | Returns |
|----------|---------|---------|
| `/get/get-albums` | `albumStore.fetchAlbums()` | All albums |
| `/get/prefetch` | `usePrefetch` | Row layout + dataLength |
| `/get/fetch-data` | grid worker | Batch of grid items |
| `/get/get-tags` | `tagStore.fetchTags()` | All tags |

The grid uses a Web Worker (background thread) to fetch item batches (`fetch-data`) without blocking the UI. The layout calculation (`prefetch`) runs first and determines how many rows exist; the worker then fills them in on demand as the user scrolls.
