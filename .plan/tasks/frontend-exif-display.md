---
status: open
type: feature
priority: medium
area: frontend
---

Expand the metadata panel to show more EXIF fields.

- `ItemExif.vue` (inside `MetadataContent.vue`): add Date taken, focal length, f-number, ISO, shutter speed
- `ItemDate.vue`: verify it shows capture date from EXIF, not the DB index date — fix if wrong
- `description` field: already stored and returned by API; verify it is displayed and editable via `PUT /put/set_user_defined_description`

Fields are already extracted by the backend at index time via `kamadak-exif`. This is a display-only frontend change.
