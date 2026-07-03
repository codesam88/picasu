# Fullscreen Lightbox Overlay — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the image view lightbox into a fullscreen overlay with a solid black background. The info sidepane opens at the right edge of the screen, full viewport height, and the image re-centers in the remaining space.

**Architecture:** Single-component change to `ViewPage.vue`. Remove the 80% viewport sizing constraint (`modalStyle`), switch the overlay background from semi-transparent to solid black, and restructure the layout as a flex row so the sidepane sits at the screen's right edge when open.

**Tech Stack:** Vue 3 + Vuetify, scoped CSS

## Global Constraints

- The mobile layout (width <= 720px) must remain unchanged
- Navigation arrows (`NavigationOverlays`) use `position: absolute; left: 0 / right: 0` relative to `.view-modal` — they will automatically reposition when the modal shrinks
- The control buttons (info, close) are `position: absolute; top: 8px; right: 8px` within `.view-modal` — they will also track the modal's right edge
- No functional changes to `ViewPageMetadata.vue`, `NavigationOverlays.vue`, or `Display.vue`

---

### Task 1: Rewrite ViewPage.vue Layout for Fullscreen

**Files:**

- Modify: `frontend/src/components/View/ViewPage.vue` (template + script + style)

**Interfaces:**

- Consumes: same props and stores as before
- Produces: same interface (renders as before, only layout changes)

- [ ] **Step 1: Remove computed properties and constants that are no longer needed**

Remove from `<script>`:

```typescript
const SIDEPANE_WIDTH = 360;
const NAV_ELEMENT_SIZE = 64;

const modalStyle = computed(() => {
  const maxWidth = windowWidth.value * 0.8;
  const maxHeight = windowHeight.value * 0.8;
  const data = abstractData.value;
  const ratio =
    data &&
    (data.type === "image" || data.type === "video") &&
    data.width &&
    data.height
      ? data.width / data.height
      : 16 / 9;
  let width = maxWidth;
  let height = width / ratio;
  if (height > maxHeight) {
    height = maxHeight;
    width = height * ratio;
  }
  return { width: `${width}px`, height: `${height}px` };
});

const imageContainerStyle = computed(() => ({
  padding: `${NAV_ELEMENT_SIZE}px`,
}));

const sidepaneStyle = computed(() => {
  if (!configStore.isMobile && showMetadataPanel.value) {
    return { height: modalStyle.value.height, width: `${SIDEPANE_WIDTH}px` };
  }
  return undefined;
});

const panelOpen = computed(
  () => showMetadataPanel.value && !configStore.isMobile,
);
```

Also remove `windowWidth` and `windowHeight` refs with their resize handler if they're only used by `modalStyle`. Check if these are used elsewhere in the component — if not, remove:

```typescript
const windowWidth = ref(window.innerWidth);
const windowHeight = ref(window.innerHeight);

function handleResize() {
  windowWidth.value = window.innerWidth;
  windowHeight.value = window.innerHeight;
}

// Also remove from onMounted/onUnmounted:
//   window.addEventListener('resize', handleResize)
//   window.removeEventListener('resize', handleResize)
```

- [ ] **Step 2: Simplify the template**

Replace the current template with:

```vue
<template>
  <div class="pa-0 h-100 w-100 position-relative bg-background">
    <template v-if="index !== undefined">
      <div
        class="view-stage"
        :class="{ 'view-stage--desktop': !configStore.isMobile }"
        @click.self="handleClose"
      >
        <div class="view-container">
          <div class="view-modal">
            <NavigationOverlays
              v-if="!configStore.isMobile"
              :previous-hash="previousHash"
              :next-hash="nextHash"
              :previous-page="previousPage"
              :next-page="nextPage"
              :show="!configStore.isMobile"
            />
            <div class="view-image-container">
              <ViewPageDisplay
                :abstract-data="abstractData"
                :index="index"
                :hash="hash"
                isolation-id="mainId"
              />
            </div>
            <div class="view-modal-controls">
              <DatabaseMenu
                v-if="
                  abstractData &&
                  (abstractData.type === 'image' ||
                    abstractData.type === 'video') &&
                  route.meta.baseName !== 'share' &&
                  share === null
                "
                :database="abstractData"
                :index="index"
                :hash="hash"
                isolation-id="mainId"
              />
              <v-btn
                icon
                size="small"
                variant="text"
                aria-label="Info"
                @click="toggleMetadataPanel"
              >
                <v-icon>{{
                  showMetadataPanel
                    ? "mdi-information"
                    : "mdi-information-outline"
                }}</v-icon>
              </v-btn>
              <v-btn
                icon
                size="small"
                variant="text"
                aria-label="Close"
                @click="handleClose"
              >
                <v-icon>mdi-close</v-icon>
              </v-btn>
            </div>
          </div>
          <ViewPageMetadata
            v-if="showMetadataPanel && abstractData"
            class="view-modal-sidepane"
            :abstract-data="abstractData"
            :index="index"
            :hash="hash"
            isolation-id="mainId"
          />
        </div>
      </div>
    </template>
    <div
      v-else
      fluid
      class="pa-0 h-100 w-100 overflow-hidden position-relative"
      style="background-color: black"
    >
      <div class="d-flex align-center justify-center w-100 h-100">
        <v-progress-circular indeterminate color="primary" size="64" />
      </div>
    </div>
  </div>
</template>
```

Key template changes from current:

1. Remove `:style="!configStore.isMobile ? modalStyle : undefined"` from `.view-modal`
2. Remove `:style="!configStore.isMobile ? imageContainerStyle : undefined"` from `.view-image-container`
3. Remove `:style="!configStore.isMobile ? sidepaneStyle : undefined"` from `ViewPageMetadata`
4. Remove `panel-open` class from `.view-stage`

- [ ] **Step 3: Rewrite the CSS**

Replace all `<style scoped>` content with:

```css
.v-container::-webkit-scrollbar {
  display: none;
}

.view-stage {
  position: relative;
  height: 100%;
  width: 100%;
}

.view-stage--desktop {
  position: fixed;
  inset: 0;
  z-index: 1000;
  display: flex;
  background: black;
}

.view-container {
  display: flex;
  flex-direction: row;
  width: 100vw;
  height: 100vh;
}

.view-modal {
  position: relative;
  flex: 1 1 auto;
  min-width: 0;
  background: black;
  overflow: hidden;
  display: flex;
  align-items: center;
  justify-content: center;
}

.view-image-container {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}

.view-modal-controls {
  position: absolute;
  top: 8px;
  right: 8px;
  z-index: 1;
  display: flex;
  gap: 4px;
  background: rgba(0, 0, 0, 0.35);
  border-radius: 24px;
  padding: 2px;
}

.view-modal-sidepane {
  flex: 0 0 360px;
  height: 100vh;
  z-index: 1001;
  background: var(--v-theme-surface);
}

@media (width <= 720px) {
  .view-modal-sidepane {
    width: 100%;
    height: auto;
  }
}
```

- [ ] **Step 4: Remove unused imports and simplify the script**

After removing computed properties, the script should no longer import `ref` from Vue. Check what's still used and strip unused imports. Also remove the `handleResize` function and its `onMounted`/`onUnmounted` listeners:

In `onMounted`, the current code has:

```typescript
window.addEventListener("keydown", handleKeyDown);
window.addEventListener("resize", handleResize);
```

Change to:

```typescript
window.addEventListener("keydown", handleKeyDown);
```

In `onUnmounted`, the current code has:

```typescript
window.removeEventListener("keydown", handleKeyDown);
window.removeEventListener("resize", handleResize);
```

Change to:

```typescript
window.removeEventListener("keydown", handleKeyDown);
```

Also remove the `ref` import from Vue if it's no longer used (check if `showMetadataPanel` ref is still there — it is, so `ref` stays).

If `onMounted` and `onUnmounted` remain but only have one listener each, decide whether to keep the lifecycle hooks or inline the keyboard listener. The current pattern uses `onMounted`/`onUnmounted` — keep the pattern but remove the resize references.

- [ ] **Step 5: Verify build and tests**

```bash
just check
just test
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/View/ViewPage.vue
git commit -m "feat: fullscreen lightbox overlay with right-edge sidepane"
```
