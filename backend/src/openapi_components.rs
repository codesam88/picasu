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
                            "context": [],
                        })))
                        .build(),
                )
                .build()
                .into(),
        )
    }
}

use utoipa::Modify;
use utoipa::openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::openapi::{Components, OpenApi};

/// Registers the API's security schemes in the generated `OpenAPI` document.
///
/// utoipa's `components(...)` clause has no `securitySchemes` entry, so the
/// schemes are added through a `Modify` addon instead. The names match the
/// `security(...)` clauses used in `backend/src/router`.
pub struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut OpenApi) {
        let components = openapi.components.get_or_insert_with(Components::new);
        components.add_security_scheme(
            "cookie_jwt",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("jwt"))),
        );
        components.add_security_scheme(
            "bearer_token",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );
        components.add_security_scheme(
            "album_id",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("x-album-id"))),
        );
        components.add_security_scheme(
            "share_id",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("x-share-id"))),
        );
    }
}
