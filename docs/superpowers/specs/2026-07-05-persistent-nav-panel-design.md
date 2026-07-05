# Persistent Nav Panel

## Summary

Replace the auto-retracting overlay drawer with a manually-toggled push sidebar.
Rename from "Drawer" to "NavPanel" throughout.

## Motivation

The current left-hand drawer (`v-navigation-drawer temporary`) overlays page content
and auto-closes on outside click / navigation, which users find disorienting.
The new panel stays open until explicitly dismissed, pushing main content aside
rather than covering it.

## Layout

`App.vue` already wraps everything in `<v-app>`. `PageTemplate.vue` inserts a
`<v-layout>` between `GalleryBar` and the content area:

```
v-app
  v-main.h-screen
    PageTemplate
      GalleryBar                  (48px tall, full width)
      v-layout                    { height: calc(100vh - 48px) }
        NavPanel                  (v-navigation-drawer, no temporary)
        div.flex-grow-1           (main content, overflow: hidden)
```

- `v-layout` is a Vuetify flex container. `NavPanel` as a direct child pushes the
  flex-grow-1 div when open.
- The old `page-root` div and its hardcoded height calc are removed — the
  `v-layout` handles vertical space.

## NavPanel.vue (renamed from Drawer.vue)

**Before:**

```vue
<v-navigation-drawer v-model="showDrawer" temporary touchless width="150">
  <v-list nav>…</v-list>
  <template #append><v-list nav>…</v-list></template>
</v-navigation-drawer>
```

**After:**

```vue
<v-navigation-drawer v-model="showNavPanel" touchless width="150">
  <template #prepend>
    <div class="d-flex justify-end pa-2">
      <v-btn icon="mdi-menu" variant="text" size="small" @click="showNavPanel = false" />
      <!-- tooltip "Hide Menu" -->
    </div>
  </template>
  <v-list nav>…</v-list>
  <template #append><v-list nav>…</v-list></template>
</v-navigation-drawer>
```

Key changes:

- `temporary` removed — no auto-close on outside click
- `touchless` kept — prevents swipe gestures from closing the panel on mobile
- Close button in `#prepend` slot, aligned to the right (`justify-end`)
- The `mdi-menu` icon is the same as the toolbar hamburger, giving the appearance
  that the button "moves into" the open panel
- Tooltip "Hide Menu" on the close button

## GalleryBar.vue

The hamburger button (`mdi-menu` at top-left of toolbar) is conditionally rendered
based on panel state:

```vue
<v-tooltip
  v-if="route.meta.level === 1 && !showNavPanel"
  location="top"
  text="Menu"
>
  <template #activator="{ props }">
    <v-btn v-bind="props" @click="showNavPanel = !showNavPanel" icon="mdi-menu" />
  </template>
</v-tooltip>
```

- When `showNavPanel` is false: the hamburger is visible in the toolbar (opener)
- When `showNavPanel` is true: the toolbar hamburger is hidden; the NavPanel's
  close button is visible at the panel's top-right (closer)
- Clicking either toggles the panel

## State management

The `showNavPanel` reactive state moves from `PageTemplate.vue` (where
`showDrawer` currently lives) into a dedicated Pinia store so both `PageTemplate`
and `GalleryBar` can read/write it without `provide/inject`.

Alternatively, keep `provide/inject` but inject into GalleryBar. Since GalleryBar
already injects `showDrawer`, this is the minimal-change path — just rename.

Decision: keep provide/inject, rename `showDrawer` → `showNavPanel`.

**Default value (orientation-based):**

```
onMounted(() => {
  showNavPanel.value = window.innerWidth > window.innerHeight
})
```

- Screen wider than tall → panel expanded by default
- Screen taller than wide → panel retracted by default
- No reactive listener for orientation changes after mount — user override takes
  priority.

## Orientation / responsive

No breakpoint-based behavior beyond the mount-time default. The user can toggle
freely at any width. The `width="150"` is narrow enough to work on compact
screens when open; if needed the user can close it.

## Files changed

| File                                                        | Changes                                                                                                                                         |
| ----------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `frontend/src/components/Page/PageLayout/Drawer.vue`        | Rename to `NavPanel.vue`                                                                                                                        |
| `frontend/src/components/Page/PageLayout/NavPanel.vue`      | Remove `temporary`; keep `touchless`; add `#prepend` close button                                                                               |
| `frontend/src/components/Page/PageLayout/PageTemplate.vue`  | Replace flat drawer + page-root with `v-layout` wrapping NavPanel + content; rename `showDrawer` → `showNavPanel`; set default from orientation |
| `frontend/src/components/NavBar/GalleryBars/GalleryBar.vue` | Inject `showNavPanel`; conditionally hide hamburger when panel is open                                                                          |

## Open/close behavior

| Trigger                                  | Action                                                                              |
| ---------------------------------------- | ----------------------------------------------------------------------------------- |
| Click toolbar hamburger (panel closed)   | `showNavPanel = true`                                                               |
| Click NavPanel close button (panel open) | `showNavPanel = false`                                                              |
| Click toolbar hamburger (panel open)     | `showNavPanel = false`                                                              |
| Outside click                            | No close — panel is sticky                                                          |
| Navigation (route change)                | No auto-close — panel stays                                                         |
| Escape key                               | No behavior change — Escape handling in PageTemplate unchanged (dialogs, edit mode) |
