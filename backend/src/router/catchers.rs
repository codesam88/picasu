//! JSON error catchers for statuses Rocket itself can emit.
//!
//! Handler and guard failures already serialize through the [`AppError`]
//! responder. These catch the remainder — framework-level responses that
//! would otherwise render as Rocket's default HTML pages, which API
//! consumers cannot parse:
//!
//! - `401`: guard/handler rejections with no other responder (also
//!   silences Rocket's "No 401 catcher registered" warning).
//! - `422`: Rocket data-guard rejections — a well-formed JSON body the
//!   route's `Json<T>` guard refuses (a shape mismatch, or unknown or
//!   mistyped fields under `deny_unknown_fields`). The body is the fixed
//!   `"Unprocessable entity"` string by contract: per-field detail is not
//!   delivered.
//! - `500`: panics and uncaught internal errors. The body carries no
//!   internals by construction (fresh [`AppError`], no source).
//!
//! Deliberately no 404 catcher: the page catch-alls serve the SPA shell
//! for unknown paths, and a catcher would fight that. No
//! 400/403/405/409 catchers either: those only ever come from the
//! [`AppError`] responder, so catchers for them would be dead code.

use rocket::http::Status;
use rocket::serde::json::Json;

use crate::router::{AppError, ErrorKind};

#[catch(401)]
fn unauthorized() -> (Status, Json<AppError>) {
    (
        Status::Unauthorized,
        Json(AppError::new(ErrorKind::Auth, "Unauthorized")),
    )
}

#[catch(422)]
fn unprocessable_entity() -> (Status, Json<AppError>) {
    (
        Status::UnprocessableEntity,
        Json(AppError::new(
            ErrorKind::InvalidInput,
            "Unprocessable entity",
        )),
    )
}

// Normalize Rocket's 415 Unsupported Media Type (a format-guard rejection on a
// route whose contract treats a wrong content type as invalid input) to the
// documented 400, so the spec never needs to carry 415. See the
// spec-reconciliation triage; the catcher is the normalization point.
#[catch(415)]
fn unsupported_media_type() -> (Status, Json<AppError>) {
    (
        Status::BadRequest,
        Json(AppError::new(
            ErrorKind::InvalidInput,
            "Unsupported media type",
        )),
    )
}

#[catch(500)]
fn internal_error() -> (Status, Json<AppError>) {
    (
        Status::InternalServerError,
        Json(AppError::new(ErrorKind::Internal, "Internal server error")),
    )
}

/// The JSON error catchers mounted at `/` by the builder.
#[must_use]
pub fn generate_catchers() -> Vec<rocket::Catcher> {
    catchers![
        unauthorized,
        unprocessable_entity,
        unsupported_media_type,
        internal_error
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unauthorized_returns_json_401_without_html() {
        let (status, body) = unauthorized();
        assert_eq!(status, Status::Unauthorized);
        assert_eq!(body.kind, ErrorKind::Auth);
        assert!(!body.message.is_empty(), "catcher carries a message string");
        assert!(body.source.is_none());
    }

    #[test]
    fn unprocessable_entity_returns_json_422() {
        let (status, body) = unprocessable_entity();
        assert_eq!(status, Status::UnprocessableEntity);
        assert_eq!(body.kind, ErrorKind::InvalidInput);
        assert!(body.source.is_none());
    }

    #[test]
    fn internal_error_returns_json_500_without_internals() {
        let (status, body) = internal_error();
        assert_eq!(status, Status::InternalServerError);
        assert_eq!(body.kind, ErrorKind::Internal);
        assert!(body.source.is_none());
        let serialized = serde_json::to_string(&*body).expect("serialize");
        assert!(!serialized.contains("source"));
    }
}
