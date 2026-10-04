// The P3 input, shaped like backend/src/error.rs: every ErrorKind variant, and
// the http_status match that maps it to a rocket Status.
pub enum ErrorKind {
    NotFound,
    PermissionDenied,
    InvalidInput,
    Internal,
    Conflict,
    Database,
    Auth,
    ReadOnlyMode,
    Serialization,
}

impl AppError {
    pub fn http_status(&self) -> Status {
        match self.kind {
            ErrorKind::NotFound => Status::NotFound,
            ErrorKind::PermissionDenied => Status::Forbidden,
            ErrorKind::Auth => Status::Unauthorized,
            ErrorKind::InvalidInput => Status::BadRequest,
            ErrorKind::Conflict => Status::Conflict,
            ErrorKind::ReadOnlyMode => Status::MethodNotAllowed,
            _ => Status::InternalServerError,
        }
    }
}
