use rocket::http::{Header, Status};
use rocket::request::Request;
use rocket::response::{self, Responder};
use serde::{Serialize, Serializer};
use std::fmt;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum ErrorKind {
    NotFound,
    PermissionDenied,
    InvalidInput,
    Internal,
    Conflict,
    Database,
    IO,
    #[allow(dead_code)]
    Serialization,
    Auth,
    ReadOnlyMode,
    /// Throttling answer for brute-force attempts (429).
    ///
    /// Renamed in JSON to `"Rate Limited"` (the [`Self::as_str`] spelling);
    /// the other variants serialize as their derived names.
    #[serde(rename = "Rate Limited")]
    RateLimited,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::NotFound => "Not Found",
            ErrorKind::PermissionDenied => "Permission Denied",
            ErrorKind::InvalidInput => "Invalid Input",
            ErrorKind::Internal => "Internal Error",
            ErrorKind::Conflict => "Conflict",
            ErrorKind::Database => "Database Error",
            ErrorKind::IO => "I/O Error",
            ErrorKind::Serialization => "Serialization Error",
            ErrorKind::Auth => "Authentication Error",
            ErrorKind::ReadOnlyMode => "Read Only Mode",
            ErrorKind::RateLimited => "Rate Limited",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum ErrorStatus {
    Permanent,
    Temporary,
}

#[derive(Debug)]
pub struct AppError {
    pub kind: ErrorKind,
    pub message: String,
    pub status: ErrorStatus,
    pub context: Vec<String>,
    pub source: Option<anyhow::Error>,
    /// Seconds a client should wait before retrying (throttling answers).
    ///
    /// Serialized as camelCase `retryAfter`, omitted entirely when `None` so
    /// the wire body of non-throttled errors is unchanged.
    pub retry_after_secs: Option<u64>,
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let field_count = if self.retry_after_secs.is_some() {
            5
        } else {
            4
        };
        let mut state = serializer.serialize_struct("AppError", field_count)?;
        state.serialize_field("kind", &self.kind)?;
        state.serialize_field("message", &self.message)?;
        state.serialize_field("status", &self.status)?;
        state.serialize_field("context", &self.context)?;
        // `AppError` serializes manually (its `source` is not serializable),
        // so the `skip_serializing_if = "Option::is_none"` equivalent lives
        // here: no `retryAfter` key at all when unset.
        if let Some(secs) = self.retry_after_secs {
            state.serialize_field("retryAfter", &secs)?;
        }
        state.end()
    }
}

impl AppError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            status: ErrorStatus::Permanent,
            context: Vec::new(),
            source: None,
            retry_after_secs: None,
        }
    }

    pub fn temporary(mut self) -> Self {
        self.status = ErrorStatus::Temporary;
        self
    }

    /// Attach a `Retry-After` hint (seconds) for throttling answers.
    ///
    /// The `Responder` impl turns this into the `Retry-After` response header;
    /// the JSON body carries it as `retryAfter`.
    pub fn retry_after(mut self, secs: u64) -> Self {
        self.retry_after_secs = Some(secs);
        self
    }

    pub fn context(mut self, ctx: impl Into<String>) -> Self {
        self.context.push(ctx.into());
        self
    }

    pub fn from_err(kind: ErrorKind, err: anyhow::Error) -> Self {
        Self {
            kind,
            message: err.to_string(),
            status: ErrorStatus::Permanent,
            context: Vec::new(),
            source: Some(err),
            retry_after_secs: None,
        }
    }

    pub fn http_status(&self) -> Status {
        match self.kind {
            ErrorKind::NotFound => Status::NotFound,
            ErrorKind::PermissionDenied => Status::Forbidden,
            ErrorKind::Auth => Status::Unauthorized,
            ErrorKind::InvalidInput => Status::BadRequest,
            ErrorKind::Conflict => Status::Conflict,
            ErrorKind::ReadOnlyMode => Status::MethodNotAllowed,
            ErrorKind::RateLimited => Status::TooManyRequests,
            _ => Status::InternalServerError,
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.as_str(), self.message)?;
        for ctx in &self.context {
            write!(f, "\n  Context: {ctx}")?;
        }
        if let Some(src) = &self.source {
            write!(f, "\n  Caused by: {src:?}")?;
        }
        Ok(())
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(anyhow::Error::root_cause)
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        Self::from_err(ErrorKind::Internal, err)
    }
}

// Rocket Responder
impl<'r> Responder<'r, 'static> for AppError {
    fn respond_to(self, req: &'r Request<'_>) -> response::Result<'static> {
        let status = self.http_status();
        let retry_after_secs = self.retry_after_secs;

        // Log the error
        log::error!("API Error: {self}");

        // Also trigger the old handler for Discord notifications if needed
        crate::error::handle_app_error(&self);

        rocket::serde::json::Json(self)
            .respond_to(req)
            .map(|mut res| {
                res.set_status(status);
                if let Some(secs) = retry_after_secs {
                    res.set_header(Header::new("Retry-After", secs.to_string()));
                }
                res
            })
    }
}

// Extension trait for Result to easily convert to AppError with context
pub trait ResultExt<T> {
    fn or_raise<F, S>(self, op: F) -> Result<T, AppError>
    where
        F: FnOnce() -> (ErrorKind, S),
        S: Into<String>;

    #[allow(dead_code)]
    fn with_context<S>(self, ctx: S) -> Result<T, AppError>
    where
        S: Into<String>;
}

impl<T, E> ResultExt<T> for Result<T, E>
where
    E: Into<anyhow::Error>,
{
    fn or_raise<F, S>(self, op: F) -> Result<T, AppError>
    where
        F: FnOnce() -> (ErrorKind, S),
        S: Into<String>,
    {
        self.map_err(|e| {
            let (kind, msg) = op();
            AppError {
                kind,
                message: msg.into(),
                status: ErrorStatus::Permanent,
                context: vec![],
                source: Some(e.into()),
                retry_after_secs: None,
            }
        })
    }

    fn with_context<S>(self, ctx: S) -> Result<T, AppError>
    where
        S: Into<String>,
    {
        self.map_err(|e| AppError {
            kind: ErrorKind::Internal,
            message: "An unexpected error occurred".to_string(),
            status: ErrorStatus::Permanent,
            context: vec![ctx.into()],
            source: Some(e.into()),
            retry_after_secs: None,
        })
    }
}

// Extension trait for Option
#[allow(dead_code)]
pub trait OptionExt<T> {
    fn or_raise<F, S>(self, op: F) -> Result<T, AppError>
    where
        F: FnOnce() -> (ErrorKind, S),
        S: Into<String>;
}

impl<T> OptionExt<T> for Option<T> {
    fn or_raise<F, S>(self, op: F) -> Result<T, AppError>
    where
        F: FnOnce() -> (ErrorKind, S),
        S: Into<String>,
    {
        self.ok_or_else(|| {
            let (kind, msg) = op();
            AppError::new(kind, msg)
        })
    }
}

use anyhow::Error;

pub fn handle_error(error: Error) -> Error {
    error!("{:?}", error);
    error
}

pub fn handle_app_error(error: &AppError) {
    match error.kind {
        ErrorKind::Auth
        | ErrorKind::PermissionDenied
        | ErrorKind::NotFound
        | ErrorKind::InvalidInput
        | ErrorKind::ReadOnlyMode => return,
        _ => {}
    }
    error!("{:?}", error);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limited_kind_maps_to_429() {
        assert_eq!(
            AppError::new(ErrorKind::RateLimited, "throttled")
                .temporary()
                .http_status(),
            Status::TooManyRequests
        );
    }

    #[test]
    fn rate_limited_kind_as_str() {
        assert_eq!(ErrorKind::RateLimited.as_str(), "Rate Limited");
    }

    #[test]
    fn retry_after_builder_sets_field() {
        let err = AppError::new(ErrorKind::RateLimited, "throttled")
            .temporary()
            .retry_after(10);
        assert_eq!(err.retry_after_secs, Some(10));
    }

    #[test]
    fn retry_after_defaults_to_none_in_constructors() {
        assert_eq!(
            AppError::new(ErrorKind::RateLimited, "throttled")
                .temporary()
                .retry_after_secs,
            None
        );
        let from_err =
            AppError::from_err(ErrorKind::Internal, anyhow::anyhow!("boom")).retry_after_secs;
        assert_eq!(from_err, None);
    }

    #[test]
    fn serialized_body_omits_retry_field_when_unset() {
        let err = AppError::new(ErrorKind::RateLimited, "throttled").temporary();
        let serialized = serde_json::to_string(&err).expect("serialize");
        assert_eq!(
            serialized,
            r#"{"kind":"Rate Limited","message":"throttled","status":"Temporary","context":[]}"#
        );
    }

    #[test]
    fn serialized_body_carries_camel_case_retry_field_when_set() {
        let err = AppError::new(ErrorKind::RateLimited, "throttled")
            .temporary()
            .retry_after(10);
        let serialized = serde_json::to_string(&err).expect("serialize");
        assert_eq!(
            serialized,
            r#"{"kind":"Rate Limited","message":"throttled","status":"Temporary","context":[],"retryAfter":10}"#
        );
    }

    // Probes for the Responder header tests. `rocket::Request` has no public
    // constructor in rocket 0.5, so the only public way to exercise
    // `respond_to` end-to-end is a real dispatch through a minimal instance
    // that mounts routes owned by this test module.
    #[rocket::get("/retry-after-probe-set")]
    fn retry_after_probe_set() -> AppError {
        AppError::new(
            ErrorKind::RateLimited,
            "Too many login attempts, try again in 10s",
        )
        .temporary()
        .retry_after(10)
    }

    #[rocket::get("/retry-after-probe-unset")]
    fn retry_after_probe_unset() -> AppError {
        AppError::new(ErrorKind::RateLimited, "Too many login attempts").temporary()
    }

    fn probe_client() -> rocket::local::blocking::Client {
        let rocket = rocket::build().mount(
            "/",
            rocket::routes![retry_after_probe_set, retry_after_probe_unset],
        );
        rocket::local::blocking::Client::untracked(rocket).expect("valid rocket instance")
    }

    #[test]
    fn responder_sets_retry_after_header_status_and_body_when_set() {
        let client = probe_client();
        let resp = client.get("/retry-after-probe-set").dispatch();
        assert_eq!(resp.status(), Status::TooManyRequests);
        assert_eq!(resp.headers().get_one("Retry-After"), Some("10"));
        assert_eq!(
            resp.into_string().as_deref(),
            Some(
                r#"{"kind":"Rate Limited","message":"Too many login attempts, try again in 10s","status":"Temporary","context":[],"retryAfter":10}"#
            )
        );
    }

    #[test]
    fn responder_omits_retry_after_header_when_unset() {
        let client = probe_client();
        let resp = client.get("/retry-after-probe-unset").dispatch();
        assert_eq!(resp.status(), Status::TooManyRequests);
        assert!(resp.headers().get_one("Retry-After").is_none());
        assert_eq!(
            resp.into_string().as_deref(),
            Some(
                r#"{"kind":"Rate Limited","message":"Too many login attempts","status":"Temporary","context":[]}"#
            )
        );
    }
}
