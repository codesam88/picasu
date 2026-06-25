# Frontend Architecture Reference

## Stack Overview

The frontend is a **Vue 3 single-page application** written in **TypeScript**, built with **Vite**.

| Layer             | Technology              | Role                                              |
| ----------------- | ----------------------- | ------------------------------------------------- |
| Language          | TypeScript              | Type-safe JS; all `.ts` and `.vue` files          |
| UI framework      | Vue 3 (Composition API) | Reactive component system                         |
| Component library | Vuetify 3               | Pre-built UI widgets (`v-overlay`, `v-btn`, etc.) |
| State management  | Pinia                   | Global reactive stores shared across components   |
| Routing           | Vue Router 4            | URL ↔ component mapping, nested routes            |
| HTTP              | Axios                   | API calls to the Rust backend                     |
| Build             | Vite                    | Dev server + production bundler                   |
| Type checking     | vue-tsc                 | TypeScript checking for `.vue` files              |

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

### Props and Events

Components communicate downward via **props** (parent passes data to child) and upward via **emits** (child signals events to parent). In this codebase, Pinia stores are used heavily instead of prop-drilling for shared state.

---

## How Vue Router Works

Vue Router maps URLs to components. This app uses **nested routes**: a parent component stays on screen while a child component renders inside it.

### `<router-view>`

A `<router-view>` tag in a component's template is a **slot** where the router inserts the matched child component.

### `useRoute()` / `useRouter()`

- `useRoute()` — returns the current route object: `.params`, `.query`, `.meta`, `.name`. Reactive; updates when the URL changes.
- `useRouter()` — returns the router instance for programmatic navigation.

---

## How Pinia Stores Work

A **store** is a globally shared reactive object. Any component can read or write it; changes propagate to all components that use it.

In this codebase every store is **keyed by isolationId** so the main grid and the album grid each get their own independent instance:

```
useDataStore('mainId')  // one store instance for the level-1 page
useDataStore('subId')   // separate store instance for the level-3 album
```

Same store name + same ID = same singleton. Different ID = different instance.

---

## How Vuetify's `v-overlay` Works

`v-overlay` is a full-screen layer that renders **outside the normal component tree** (Vuetify teleports it to `<body>`). It stacks visually using z-index. Levels 2, 3, and 4 each render as overlays. Closing the overlay (ESC or back gesture) calls `router.back()`, which pops the URL back one level.

---

## Route Levels

Every page section (home, albums, all, etc.) shares a 4-level nested route structure, parameterized by `baseName`:

| Level | URL pattern                                 | Route name               | Component                          |
| ----- | ------------------------------------------- | ------------------------ | ---------------------------------- |
| 1     | `/{baseName}`                               | `{baseName}`             | Page component (e.g. `AlbumsPage`) |
| 2     | `/{baseName}/view/:hash`                    | `{baseName}ViewPage`     | `ViewPageMain`                     |
| 3     | `/{baseName}/view/:hash/read`               | `{baseName}ReadPage`     | `HomeIsolated`                     |
| 4     | `/{baseName}/view/:hash/read/view/:subhash` | `{baseName}ReadViewPage` | `ViewPageIsolated`                 |

**baseName values with full 4-level structure:** `home`, `all`, `favorite`, `archived`, `trashed`, `albums`, `videos`, `album`

**baseName values with partial structure:**
- `tags` — flat single page (`/tags`), no view/read overlays
- `share` — 2 levels (`/share/:albumId-:shareId` and `/share/:albumId-:shareId/view/:hash`)
- `config` — flat single page (`/config`)
- `links` — flat single page (`/links`)
- `login` — flat single page (`/login`)

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
                                └── Display.vue
                                    ├── DisplayDesktop / DisplayMobile → media viewer
                                    └── DisplayAlbum.vue
                                        └── <router-view>  ← renders level 3
                                            └── HomeIsolated  [v-overlay]
                                                └── Home.vue  (isolation-id="subId")
                                                    └── <router-view>  ← renders level 4
                                                        └── ViewPageIsolated  (isolation-id="subId")
                                                            └── ViewPage.vue  [v-overlay]
                                                                └── Display.vue
                                                                    ├── DisplayDesktop/DisplayMobile
                                                                    └── DisplayAlbum.vue
                                                                        └── <router-view>  (no level 5)
```

---

## Key Components

### Home.vue

The reusable infinite-scroll photo grid. Used at both level 1 (main page) and level 3 (inside an album).

- **Props:** `basicString` (filter query), `isolation-id`, `searchString`
- **Contains:** a `<router-view>` that the next overlay level renders into
- **Key behavior:** calls `usePrefetch` on mount to fetch rows from the backend

### ViewPage.vue

Full-screen v-overlay that shows a single item (photo, video, or album detail).

- Receives `isolation-id` from its parent
- Looks up `:hash` (level 2) or `:subhash` (level 4) in `dataStore(isolationId)`
- Renders `Display.vue` which conditionally shows media or album content

### DisplayAlbum.vue

Shown inside `ViewPage.vue` when the current item is an album.

- Shows cover image and metadata card
- Shows an **"Enter Album"** button at level 2, which navigates to level 3
- Contains a `<router-view>` — this is how `HomeIsolated` (level 3) renders on top

### HomeIsolated.vue

Level 3 component: a full-screen v-overlay wrapping `Home.vue` with a filter scoped to one album.

### HomeTemp.vue

A third overlay scope (`isolation-id="tempId"`) used for the move-to-album selection dialog. Provides a separate grid instance for browsing albums during item moves.

---

## Data Stores (Pinia, keyed by isolationId)

| Store             | Key              | Contents                                                                                  |
| ----------------- | ---------------- | ----------------------------------------------------------------------------------------- |
| `dataStore`       | `mainId`         | Grid items (photos/videos/albums) matching the level-1 page filter                        |
| `dataStore`       | `subId`          | Grid items matching the current album filter (level 3)                                    |
| `albumStore`      | `mainId`         | **All** albums from `/get/get-albums` — keyed by `albumId`. Shared across all components. |
| `prefetchStore`   | `mainId`/`subId` | Scroll/prefetch state for each grid                                                       |
| `collectionStore` | `mainId`/`subId` | Selected items (edit mode)                                                                |

`albumStore('mainId')` is fetched once per session and contains every album including sub-albums. `dataStore` only contains what the current page's `basicString` filter returns.

Additional stores exist for configuration (`configStore`), messages (`messageStore`), uploads (`uploadStore`), tags (`tagStore`), and other concerns — ~28 stores total.

---

## Isolation IDs

| ID       | Used in                         | Scope                        |
| -------- | ------------------------------- | ---------------------------- |
| `mainId` | Levels 1–2 (main grid + viewer) | Top-level page                |
| `subId`  | Levels 3–4 (album grid + viewer)| Inside an open album          |
| `tempId` | `HomeTemp`                      | Move-to-album selection modal |

---

## Navigation Rules

| From level       | Click on             | Navigates to                     | Route name                  |
| ---------------- | -------------------- | -------------------------------- | --------------------------- |
| 1 (grid)         | photo/video          | level 2 viewer                   | `{baseName}ViewPage`        |
| 1 (grid)         | album (any page)     | level 3 album grid               | **always `albumsReadPage`** |
| 2 (DisplayAlbum) | "Enter Album" button | level 3 album grid               | `{baseName}ReadPage`        |
| 3 (album grid)   | photo/video          | level 4 viewer                   | `{baseName}ReadViewPage`    |
| 3 (album grid)   | sub-album            | level 3 album grid for sub-album | **always `albumsReadPage`** |
| any              | back gesture / ESC   | one level up                     | `router.back()`             |

---

## Filter System

Filters are string expressions parsed by the Chevrotain lexer (`lexer.ts`). Each page sets a `basicString` that determines what `dataStore` contains:

| Page          | basicString                                                 |
| ------------- | ----------------------------------------------------------- |
| HomePage      | `and(not(type:"album"), archived:false, trashed:false)`      |
| AllPage       | `trashed:false`                                              |
| FavoritePage  | `and(favorite:true, trashed:false)`                          |
| ArchivedPage  | `and(archived:true, trashed:false)`                          |
| TrashedPage   | `trashed:true`                                               |
| AlbumsPage    | `and(type:"album", trashed:false, root_album:true)`          |
| VideosPage    | `and(type:"video", archived:false, trashed:false)`           |
| HomeIsolated  | `and(trashed:false, or(album:"<id>", parent_album:"<id>"))`  |

`parent_album:"<id>"` matches album objects whose directory parent is the given album. It is always false for images/videos.

---

## Config Page (`/config`)

The config page at `ConfigPage.vue` contains:

| Section          | Component         | Features                                                    |
| ---------------- | ----------------- | ----------------------------------------------------------- |
| Display          | `FrontendConfig`  | Frontend-only UI preferences (light/dark mode, chips, etc.) |
| Password         | `ChangePassword`  | Set or change the admin password                            |
| Image Paths      | `StorageAndSync`  | Monitored path display, Scan Now button, scan status with counters (scanned/matched/processed/failed), Cancel button. Also has editable `maxUploadSize` and `uploadFolder` fields. Save writes to `PUT /put/config`. |
| Advanced Settings | `AdvancedConfig`  | Read-only mode toggle, disable image processing, JWT auth key |

---

## API Calls (frontend → backend)

| Endpoint               | Used by                              | Returns                 |
| ---------------------- | ------------------------------------ | ----------------------- |
| `/get/config`          | `configStore.fetchConfig()`          | Public config (camelCase JSON) |
| `/put/config`          | `configStore.updateConfig()`         | —                       |
| `/put/config/password` | `ChangePassword`                     | —                       |
| `/get/get-albums`      | `albumStore.fetchAlbums()`           | All albums              |
| `/get/prefetch`        | `usePrefetch`                        | Row layout + dataLength |
| `/get/get-data`        | grid worker (`toDataWorker.ts`)      | Batch of grid items     |
| `/get/get-rows`        | grid worker (`toDataWorker.ts`)      | Row layout calculation  |
| `/get/get-tags`        | `tagStore.fetchTags()`               | All tags                |
| `/get/index/status`    | `StorageAndSync.vue` (scan polling)  | Album index status      |
| `/post/index/album`    | `StorageAndSync.vue` (scan trigger)  | —                       |
| `/post/index/cancel`   | `StorageAndSync.vue` (cancel scan)   | —                       |

The grid uses a Web Worker (`toDataWorker.ts`) to fetch item batches via `/get/get-data` without blocking the UI. The layout calculation (`/get/get-rows`) runs first and determines how many rows exist; the worker then fills them in on demand as the user scrolls.
