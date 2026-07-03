---
status: open
type: feature
priority: medium
area: frontend
---

## UI Refinement

Consolidated from `ui-notes.md` — remaining work after nav/settings cleanup.

### Navigation

- [ ] **Auto-hide nav behavior**: only collapse on very slim screens; otherwise keep fixed with a toggle button to switch modes.
- [ ] **Breadcrumbs**: show current album/image path in the top bar or a nav directory tree.

### Search / Filter bar

- [ ] On click, show a dropdown to help build filters (tag picker, album picker, property toggles).
- [ ] Unify global photo properties (favorite, trashed) as the same filter type, selectable via the search bar dropdown.

### Album view

- [ ] Album properties in grid view: allow setting a fancy name and custom cover image.
- [ ] Quick favorite labeling from grid view.

### Config / Settings

- Rework "Image Path" and "Album Index":
  - [x] On first start (empty DB), set up a watcher on `IMAGE_HOME`.
  - [x] Offer to index `IMAGE_HOME` from the main page.
  - [ ] Detect whether FS notify is available; fall back to manual indexing on networked filesystems.
- [x] Delete flow: mark as trashed (visible in trash bin), require confirmation on final delete.
- [ ] Smart move: discard if identical target file exists, auto-rename otherwise.

### Misc

- [x] Verify image move operation works correctly.
- [ ] Verify album move operation works correctly.

### UI bugs

- [ ] All dialogs should be closable using Escape
- [ ] Allow right-click on grid item to open properties
- [ ] Image View has two back buttons in top left corner
- [ ] Back/Escape from Image view goes to blank grid, should go back to album
- [ ] Back/Escape from Album should go to parent Album

## UI definition

### Main Navi Items

- Timeline
- Albums
- Tags
- Personal? (User settiings, User albums)
- Shared? (Global settings, global albums)

### Main Dialogs

- Assign Album / Move Album
  - Move A to B...selection dialog within HOME
  - Expand only current selected Album, collapse others
  - SkipExisting or AutoRename
    - Detailed renaming settings in /config

- UploadImage
  - From HOME or any Album, select multiple sources, upload to here
  - Don't actually need to define an upload folder...its for the backup app or watcher disabled case

- ViewAlbum
  - select preview image
  - select custom pretty title
  - other items unused, deprecate API endpoints?

- ViewImage
  - view/set custom title, description, tags...
  - favorite rating
  - etc.

- Edit / Batch Edit Modal
  - move
  - rotate
  - add tag
  - delete
  - share?
  - ...?

- Backend Settings / Global
  - scanning options
  - autorename pattern
  - ...?

- User Settings / Personal
  - frontend settings
    - light/dark
    - preview size
  - password
  - ...?
