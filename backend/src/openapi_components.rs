//! Reusable `OpenAPI` response components shared by multiple operations.
//!
//! Components live here so that one contract detail (for example the `401`
//! behavior of every guarded route) is documented in exactly one place.
//! [`crate::openapi`] registers them via `components(responses(...))` — the
//! registration list is emitted by `backend/build.rs`.

/// The shared `401 Unauthorized` response, referenced from `#[utoipa::path]`
/// annotations as `(status = 401, response = Unauthorized)`.
///
/// Operations behind an authentication guard (`GuardAuth`, `GuardShare`,
/// `GuardTimestamp`, `GuardHash`, `GuardHashOriginal`, `GuardUpload`,
/// `TimestampGuardModified`) or that reject invalid credentials in the handler
/// body return this response.
///
/// `GuardReadOnlyMode` is deliberately not covered: it answers `405`, not `401`.
pub struct Unauthorized;

impl<'__r> utoipa::ToResponse<'__r> for Unauthorized {
    fn response() -> (
        &'__r str,
        utoipa::openapi::RefOr<utoipa::openapi::response::Response>,
    ) {
        (
            "Unauthorized",
            utoipa::openapi::ResponseBuilder::new()
                .description(
                    "Authentication credentials are missing, malformed, expired, or invalid for this operation.",
                )
                .build()
                .into(),
        )
    }
}
