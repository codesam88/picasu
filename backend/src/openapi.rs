//! The `OpenAPI` document.
//!
//! `#[utoipauto]` walks `src/router` at compile time and hands `#[derive(OpenApi)]`
//! every `__path_*` item it finds there, so a handler reaches `paths(...)` by
//! carrying `#[utoipa::path]` — there is no list of modules and no generated file
//! to keep in sync. Route-set parity against the real mount table is
//! `--check-openapi` (`crate::openapi_parity`).

use crate::openapi_components::Unauthorized;
use utoipa::OpenApi;
use utoipauto::utoipauto;

#[utoipauto(paths = "./backend/src/router")]
#[derive(OpenApi)]
#[openapi(
    components(responses(Unauthorized)),
    info(
        description = "Self-hosted photo gallery API. This document is generated from the `#[utoipa::path]` annotations in `backend/src/router`; see `docs/openapi-generator.md`.",
        contact(name = "picasu", url = "https://github.com/codesam88/picasu")
    ),
    tags(
        (name = "auth", description = "Authentication and token renewal"),
        (name = "albums", description = "Albums and shares: creation, assignment, covers, titles, descriptions, share links"),
        (name = "assets", description = "Per-asset metadata and editing: flags, rating, tags, rotation, thumbnails, deletion"),
        (name = "config", description = "Server configuration: read, write, password, export/import, path completion"),
        (name = "index", description = "Filesystem indexing jobs and full rebuild"),
        (name = "serving", description = "Media byte delivery (compressed and original files)"),
        (name = "timeline", description = "Grid/list data: prefetch, rows, scrollbar, tag list, export"),
        (name = "upload", description = "File upload"),
        (name = "pages", description = "SPA HTML page routes served from `router/get/get_page.rs`"),
        (name = "internal", description = "Operations outside the published API (stripped from this document)")
    )
)]
pub struct ApiDoc;

/// # Panics
/// Panics if `ApiDoc::openapi().to_json()` fails.
#[must_use]
pub fn generate_json() -> String {
    ApiDoc::openapi()
        .to_json()
        .expect("OpenAPI serialization failed")
}
