---
status: done
type: chore
priority: medium
area: tooling
---

Split the `xtask` workspace member into two focused crates under `utils/` and
extend the image generator with a fast minimal mode for playwright fixtures.

## Background

`xtask` currently bundles two unrelated tools:

- `test-image` — generates synthetic JPEGs/PNGs with EXIF/XMP/IPTC metadata.
  Used as a **library** by `backend` (dev-dep) and as a **binary** called via
  `spawn('cargo', ['xtask', 'test-image', ...])` in `executeGiven.ts`.
- `plan` — views and validates `.plan/` task YAML files. Pure developer
  workflow, no relation to testing.

Mixing them means touching `plan` invalidates `test-image`'s incremental cache
and vice versa.

## Scope

### 1. Extract `utils/snapfab`

Move all image-generation code from `xtask/src/test_image.rs` into a new crate
`utils/snapfab` with both `[lib]` and `[[bin]]` targets.

- Add `utils/snapfab` to workspace `members` in root `Cargo.toml`
- Update `backend/Cargo.toml` dev-dependency: `xtask` → `snapfab = { path = "../utils/snapfab" }`
- Update all `use xtask::test_image::…` imports in `backend/src/tests/` to `use snapfab::…`
- Update `executeGiven.ts` spawn call:
  `['xtask', 'test-image', 'batch', '-']` → `['run', '-p', 'snapfab', '--quiet', '--', 'batch', '-']`
- Update justfile references to `cargo xtask test-image` accordingly

### 2. Extract `utils/plan`

Move `xtask/src/plan.rs` into a new crate `utils/plan` with a `[[bin]]` target
only (no library surface needed).

- Add `utils/plan` to workspace `members`
- Update justfile: `cargo xtask plan` → `cargo run -p plan --`
- No other crates depend on `plan` as a library

### 3. Drop `xtask`

Once both tools have their own home, remove the `xtask` workspace member
entirely. Remove `xtask/` directory.

### 4. Add `--minimal` mode to `snapfab`

Playwright tests need valid image files with EXIF tags embedded, but nobody
ever looks at them. The render step (Mandelbrot, Julia, landscapes…) is
unnecessary overhead for playwright fixtures where `width` and `height` are
already forced to 4×4.

Add a `--minimal` flag (or `format: "minimal"` in the JSON spec) that:

- Skips all render modes — uses a hardcoded 1×1 or 2×2 solid-colour pixel
  instead of running `RgbImage::from_fn`
- Still runs the full EXIF/XMP/IPTC embedding pipeline (this is the part that
  actually exercises Picasu's metadata extraction)
- Produces a valid JPEG accepted by `image` and `kamadak-exif`

The `generate_batch` library entry point (called from Rust backend tests) and
the `batch` CLI subcommand (called from `executeGiven.ts`) should both respect
this flag. Backend integration tests already specify `width: Some(4), height:
Some(4)` — consider making `--minimal` the default when both dimensions are ≤ 4.

`PhotoSpec` change:

```rust
pub struct PhotoSpec {
    // existing fields …
    #[serde(default)]
    pub minimal: bool,   // skip render, use solid pixel
}
```

CLI change: add `--minimal` / `-m` flag to `single` and `batch` subcommands,
and propagate it through `generate_photo`.

## Update `executeGiven.ts`

Pass `minimal: true` in every spec emitted for playwright tests (all entries in
`photoManifest` constructed from `photo` and `dir_album` given items). The
`photo_raw` path in `executeGiven.ts` that intentionally leaves files unindexed
should also use minimal mode.

## Non-goals

- Do not change the `PhotoSpec` JSON schema in a way that breaks existing
  scenario YAML files — `minimal` must default to `false`.
- Do not remove any render mode — `library` subcommand and manual `single` use
  still want realistic output.
- Do not add `--quiet` globally; only the `executeGiven.ts` spawn needs it to
  suppress compile output in test logs.

## Files touched

| File                                        | Change                                   |
| ------------------------------------------- | ---------------------------------------- |
| `Cargo.toml` (root)                         | members: add snapfab, plan; remove xtask |
| `utils/snapfab/`                            | new crate (moved from xtask)             |
| `utils/plan/`                               | new crate (moved from xtask)             |
| `xtask/`                                    | deleted                                  |
| `backend/Cargo.toml`                        | dev-dep xtask → snapfab                  |
| `backend/src/tests/`                        | update use paths                         |
| `frontend/tests/playwright/executeGiven.ts` | spawn path + minimal flag                |
| `justfile`                                  | update cargo run -p invocations          |

## Progress

- 2026-09-24: Closed. Scope items 1, 3, 4, and `executeGiven.ts` landed as specified (`utils/snapfab`, `xtask/` removed,
  `PhotoSpec.minimal`, spawn via `cargo run -p snapfab`). Scope item 2 (`utils/plan`) was not built: the plan CLI now
  comes from the external `tablethat` tool (see `.cargo/config.toml` and justfile install recipe).
