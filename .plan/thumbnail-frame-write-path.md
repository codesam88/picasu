---
status: idea
type: bug
priority: medium
area: backend
---

## Notes

Discovered during the 2026-10-04 description backfill of
`put/regenerate_thumbnail_with_frame`: the handler writes the uploaded frame to
`object/compressed/<first two id chars>/<asset_id>.jpg`, while
`AbstractData::compressed_path()` — the path the `GuardHash`/`get-img` serving
route resolves — keys compressed thumbnails by the asset's content `hash`.
`AssetRecord.asset_id` and `AssetRecord.content_hash` are independent generated
values, so the frame may land where the serving route never looks.

Whether the frame is unservable depends on how the frontend builds the
compressed-file URL; that was not traced. First step: follow the frontend's
thumbnail URL construction and check it against the write path, then either fix
the write key or document why both keys coincide.

The handler's new doc comment records the write path exactly as coded and makes
no claim about reachability.
