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
#[openapi(components(responses(Unauthorized)))]
pub struct ApiDoc;

/// # Panics
/// Panics if `ApiDoc::openapi().to_json()` fails.
#[must_use]
pub fn generate_json() -> String {
    ApiDoc::openapi()
        .to_json()
        .expect("OpenAPI serialization failed")
}
