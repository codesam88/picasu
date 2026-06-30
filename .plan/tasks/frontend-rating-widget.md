---
status: open
type: feature
priority: high
area: frontend
---

Show and edit the `rating` field (0–5 stars) in the metadata panel.

- Read: `abstractData.rating` is now returned by the API (null or 0–5)
- Display: star widget in `MetadataContent.vue` (read-only when not owner, editable otherwise)
- Edit: clicking a star calls `PUT /put/edit_rating` with `{ indexArray, timestamp, rating }`
- Clearing: clicking the active star sets rating to null

Backend endpoint and DB field are complete. This is purely a frontend addition.
