//! Reusable `OpenAPI` response components shared by multiple operations.
//!
//! Components live here so that one contract detail (for example the `401`
//! behavior of every guarded route) is documented in exactly one place.
//! [`crate::openapi`] registers them via the `components(...)` entry
//! in its `#[openapi(...)]` attribute.

use utoipa::openapi::{ContentBuilder, Ref, ResponseBuilder};

/// The JSON body of the shared `401 Unauthorized` response.
///
/// Every 401 carries an `AppError` serialization: `kind` is always `"Auth"`,
/// `status` is always `"Permanent"` (throttling answers are 429, never 401),
/// and `retryAfter` never appears on a 401. `message` is the fixed
/// `"Unauthorized"` string for framework rejections answered by the 401
/// catcher, and the handler's reason for credential rejections raised in a
/// handler body.
#[derive(utoipa::ToSchema)]
pub struct UnauthorizedBody {
    /// The error kind, always `"Auth"` on a 401.
    #[schema(example = "Auth")]
    pub kind: String,
    /// The fixed `"Unauthorized"` string for framework rejections; the
    /// handler's reason for credential rejections raised in a handler.
    #[schema(example = "Unauthorized")]
    pub message: String,
    /// The error status, always `"Permanent"` on a 401.
    #[schema(example = "Permanent")]
    pub status: String,
    /// Empty on framework rejections; handler context otherwise.
    pub context: Vec<String>,
}

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
            ResponseBuilder::new()
                .description(
                    "Authentication credentials are missing, malformed, expired, or invalid for this operation.",
                )
                .content(
                    "application/json",
                    ContentBuilder::new()
                        .schema(Some(Ref::from_schema_name(
                            <UnauthorizedBody as utoipa::ToSchema>::name(),
                        )))
                        .example(Some(serde_json::json!({
                            "kind": "Auth",
                            "message": "Unauthorized",
                            "status": "Permanent",
                            "context": [],
                        })))
                        .build(),
                )
                .build()
                .into(),
        )
    }
}
