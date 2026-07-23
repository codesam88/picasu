---
status: backlog
type: feature
priority: medium
area: backend
---

Replace `PUT /put/regenerate-thumbnail-with-frame` (multipart frame upload) with
a JSON endpoint that accepts a video timestamp for server-side frame extraction.

## Motivation

- Eliminates the other multipart endpoint (only `POST /upload` would remain)
- Makes the endpoint testable via the YAML DSL (JSON body)
- No frame data transfer — client sends a few bytes
- Server-side extraction via ffmpeg is consistent vs browser canvas capture
- ffmpeg is already a hard dependency (checked at startup in `init.rs`)

## Backend

- New endpoint: `PUT /put/set-video-thumbnail-timestamp` with `Json<{ hash: String, timestampMs: u64 }>`
- Server extracts frame via `ffmpeg -ss <timestampMs/1000> -i <source> -vframes 1 -vf scale=W:H <thumbnail.jpg>`
- Regenerate thumbhash + phash from the new thumbnail
- Remove `PUT /put/regenerate-thumbnail-with-frame` and `RegenerateThumbnailForm`

## Frontend

- Replace `ItemRegenerateThumbnailByFrame.vue` (canvas capture + multipart upload)
  with a component that reads the video player's current time and sends a JSON request
- Remove `currentFrameStore` dependency if no longer needed elsewhere

## Testing

- YAML scenario: index a video → `PUT /put/set-video-thumbnail-timestamp` → verify thumbnail changed
- YAML scenario: verify the old endpoint returns 404/410
