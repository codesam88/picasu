---
status: done
type: chore
priority: medium
area: backend
---

## Backend refactoring: module restructure

Consolidated `backend/src/` modules for clarity. Eliminated the `public/` and
`operations/` top-level directories. Distributed their contents to `model/`,
`storage/`, `process/`, `router/`, and top-level files (commit 9cd3a2fa7).

### New layout

```
backend/src/
├── constant.rs        ← ROW_BATCH_NUMBER, VALID_IMAGE_EXTENSIONS, etc.
│                        (was public/constant/mod.rs re-exports)
├── error.rs           ← AppError, ErrorKind, handle_error / handle_app_error
│                        (was public/error.rs + public/error_data.rs)
├── frontend.rs        ← FrontendAssets struct
│                        (was public/embedded.rs)
├── init.rs            ← boot sequence: config init, logger, FFmpeg check,
│                        DB cache clear
│                        (was operations/initialization/* +
│                         process/initialization.rs)
├── model/
│   ├── abstract_data.rs  ← was public/structure/abstract_data.rs
│   ├── album.rs          ← was public/structure/album/* (4 files)
│   ├── config.rs         ← was public/structure/config.rs + env-var
│   │                        overrides + TOML section support
│   ├── expression.rs     ← was public/structure/expression/* (3 files) +
│   │                        generate_filter.rs + generate_filter_hide_metadata.rs
│   ├── image.rs          ← was public/structure/image/* (3 files)
│   ├── media.rs          ← was public/media.rs
│   ├── object.rs         ← was public/structure/object.rs
│   ├── response.rs       ← was public/structure/response/* (4 files) +
│   │                        structure/common/file_modify.rs
│   └── video.rs          ← was public/structure/video/* (3 files)
├── storage/
│   ├── db.rs             ← permanent DB: TREE (index_v5.redb) + open_data_table
│   │                        (was public/db/tree/* + operations/open_db/ +
│   │                         public/constant/redb.rs)
│   ├── cache.rs          ← transient DBs: TREE_SNAPSHOT (temp_db),
│   │                        QUERY_SNAPSHOT (cache_db), EXPIRE (expire_db)
│   │                        (was public/db/tree_snapshot/* +
│   │                         query_snapshot/* + expire/*)
│   ├── files.rs          ← path resolution
│   │                        (was public/constant/storage.rs +
│   │                         operations/utils/image_path.rs)
│   └── ser_de.rs         ← versioned codec
│                           (was public/constant/ser_de.rs)
├── process/
│   ├── dir_album.rs      ← was operations/dir_album.rs
│   ├── hash.rs           ← was operations/hash/
│   ├── index.rs          ← was process/info.rs (renamed)
│   ├── xmp.rs            ← was operations/extract_keywords.rs
│   ├── exif.rs           ← was operations/generate_exif.rs
│   ├── transitor.rs      ← was operations/transitor/
│   ├── misc.rs           ← was operations/indexation/* (generate_ffmpeg,
│   │                        generate_image_hash, generate_width_height,
│   │                        fix_orientation, generate_dynamic_image) +
│   │                        operations/utils/resize.rs
│   ├── thumbnail.rs      ← was operations/indexation/generate_thumbnail.rs
│   └── video.rs          ← was operations/indexation/generate_compressed_video.rs
│                           + operations/indexation/video_ffprobe.rs (combined)
├── router/
│   ├── auth.rs           ← claims/ + fairing/ merged (913 lines)
│   │                        (was router/claims/* + router/fairing/*)
│   ├── cache.rs          ← renamed from router/fairing/cache_control_fairing.rs
│   ├── delete.rs         ← flattened from router/delete/delete_data.rs
│   ├── get/              ← 9 handlers (unchanged)
│   ├── post/             ← 6 handlers (unchanged)
│   └── put/              ← 11 handlers (unchanged)
├── tasks/                ← background task framework (mostly unchanged)
│   ├── runtime.rs        ← tokio runtimes + rayon pool
│   │                        (was public/constant/runtime.rs)
│   ├── actor/            ← 8 task types
│   ├── batcher/          ← 7 batch tasks
│   └── looper/           ← periodic loop
└── tests/                ← unchanged
```

### Dependency layering

| Module                | Depends on                                                  | Issues                                               |
| --------------------- | ----------------------------------------------------------- | ---------------------------------------------------- |
| `model/abstract_data` | `model::object`                                             | clean                                                |
| `model/config`        | `storage::files`, `tasks::batcher::reload_watcher`          | legitimate — config save reloads watcher             |
| `model/expression`    | `process::dir_album`                                        | legitimate — filter evaluation needs dir-album cache |
| `storage/db`          | `model::*`                                                  | clean                                                |
| `storage/cache`       | `model::*`, `storage::db`                                   | clean                                                |
| `storage/files`       | nothing in crate                                            | pure filesystem path logic                           |
| `storage/ser_de`      | `model::*`                                                  | clean                                                |
| `process/index`       | `process::*`, `storage::files`, `tasks::*`                  | clean                                                |
| `tasks/actor`         | `process::*`, `storage::db`, `tasks::*`                     | clean                                                |
| `tasks/batcher`       | `process::dir_album`, `storage::db`, `model::*`, `tasks::*` | clean                                                |
| `router/auth`         | `model::*`, `router::*`                                     | clean                                                |

### Resolved issues

1. **`model/abstract_data` → `process/hash` circular dep** —
   `generate_random_data()` on `AbstractData` was dead code after the
   `random::generate_random_data` endpoint was removed. Deleted the method;
   the only remaining caller of `generate_random_hash()` is `process/dir_album.rs`,
   which is in the same layer. No layering violation remains.

2. **`storage/ser_de` → `router/get/get_prefetch` circular dep** —
   `Prefetch` was moved from the router layer to `model/response.rs`.

### Accidental artifact

- `backend/src/lib.rs.bak` — removed.

### File sizes

```
>200 lines:  abstract_data 653, ser_de 617, expression 768, config 487,
             cache 417, album 288, error 247, dir_album 250,
             init 184, video 185, start_watcher 181, misc 163,
             response 147, db 140, xmp 140, files 111

100-200:     index 116, transitor 91, exif 92, auth 913, album_index 316
<100:        thumbnail 48, hash 30, constant 18, frontend 5, media 13,
             remove_tree_snapshot 54, open_file 63, deduplicate 100,
             flush_tree 70, flush_tree_snapshot 87, flush_query_snapshot 92,
             expire_check 79, update_expire 47, update_tree 85,
             looper 47, edit_* (various 100-180)
```
