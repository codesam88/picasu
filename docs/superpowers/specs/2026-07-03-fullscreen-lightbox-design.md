# Fullscreen Lightbox Overlay

## Problem

The image view lightbox centers the image in a modal that occupies 80% of the viewport, with a semi-transparent overlay behind it. The info sidepane floats alongside the modal box. This feels constrained — photos should fill more of the screen in a lightbox, and the sidepane should integrate cleanly at the edge.

## Current Behavior

- `ViewPage.vue` renders as a fixed-position overlay (`z-index: 1000`, `inset: 0`, `rgba(0,0,0,0.35)` background)
- The image modal is sized to 80% of viewport dimensions, aspect-ratio-fitted via `modalStyle` computed property
- The sidepane (`ViewPageMetadata`, 360px) sits alongside the modal in a flex row
- When the sidepane opens, the `.view-stage` fixed box gets `right: 360px` via `.panel-open` class, shrinking the available area
- The sidepane height matches the modal's computed height

## Design

**Fullscreen lightbox:** Replace the 80% centered modal with a solid-black fullscreen area. The image and its surrounding black canvas fill the viewport from edge to edge.

**Centered image:** The image remains centered within the available black area. When no sidepane is open, the black area is the full viewport. When the sidepane opens, its 360px width is subtracted from the right side, and the image re-centers within the remaining space.

**Sidepane at right edge:** The info pane attaches to the right edge of the screen and spans the full viewport height, regardless of image dimensions.

**Navigation arrows:** Positioned at the left and right edges of the black image area (not the viewport edge), so they shift left when the sidepane opens.

**Control buttons:** Positioned at the top-right of the black image area (before the sidepane).

## Layout Architecture

```
┌─────────────────────────────────┬──────────────┐
│                                 │              │
│        Black image area         │   Sidepane   │
│     (image centered here)       │   (360px)    │
│                                 │   (hidden    │
│  ◀  [image]  ▶                  │    until     │
│               ╭───╮             │   toggled)   │
│               │ ⓘ │             │              │
│               │ ✕ │             │              │
│               ╰───╯             │              │
│                                 │              │
└─────────────────────────────────┴──────────────┘
  ← flex: 1 (black bg, black) →   ← 360px fixed →
```

When sidepane is closed, the right column is not rendered and black area fills 100vw.

## Implementation

### Files Affected

| File                                        | Changes                                                                                  |
| ------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `frontend/src/components/View/ViewPage.vue` | Rewrite layout, remove `modalStyle`, change background, restructure sidepane positioning |

### Layout Changes (ViewPage.vue)

**Structure:** The `.view-container` becomes a `display: flex; flex-direction: row` container at 100vw x 100vh.

1. **`.view-modal`** (black bg): `flex: 1 1 auto; min-width: 0` — fills available width, centers the image
2. **`.view-modal-sidepane`**: `flex: 0 0 360px; height: 100vh` — fixed 360px column at right edge, rendered only when `showMetadataPanel` is true
3. **`.view-stage--desktop`**: `background: black` (solid), `position: fixed; inset: 0`

**Navigation arrows** (`NavigationOverlays`): Keep their current position mechanism relative to `.view-modal`, which automatically shifts left when the flex container allocates space to the sidepane.

**Control buttons** (`.view-modal-controls`): Remain `position: absolute; top: 8px; right: 8px` within `.view-modal`, so they track with the image area's right edge.

### CSS Changes

| Rule                   | Change                                                            |
| ---------------------- | ----------------------------------------------------------------- |
| `.view-stage--desktop` | `background: black` (from `rgba(0,0,0,0.35)`)                     |
| `.view-stage--desktop` | `inset: 0` (keep)                                                 |
| `.view-stage--desktop` | Remove `transition: right 0.2s ease`                              |
| `.view-modal`          | Remove `modalStyle` computed — fill container instead             |
| `.view-container`      | `display: flex; flex-direction: row; width: 100vw; height: 100vh` |
| `.view-modal-sidepane` | `height: 100vh` (from matching modal height)                      |

### Removals

- `modalStyle` computed property and its usage
- `sidepaneStyle` computed property (no longer needed — sidepane is fixed via flex)
- `panelOpen` computed class toggle on `.view-stage`
- `imageContainerStyle` (nav padding no longer needed as separate concept — the image fills full area)

### Edge Cases

- **Mobile:** No change — already uses a separate layout via `DisplayMobile` and full-width sidepane at 720px breakpoint
- **Very wide images (panoramas):** The image fills height and centers horizontally within the black area — same `object-fit: contain` behavior as today
- **Very tall images:** The image fills width and centers vertically
- **Keyboard navigation:** Arrow keys for prev/next work the same — the listener is on `Display.vue`, not affected

## Verification

- `npm audit --omit=dev` passes
- `just check` passes
- `just test` passes
- Desktop: image fills viewport, centered, solid black background
- Desktop: toggle info button opens sidepane at right edge, full height
- Desktop: image re-centers in remaining space when sidepane opens
- Desktop: navigation arrows position at edges of image area
- Desktop: close button exits view
- Mobile: unchanged behavior
