use anyhow::Error;

use crate::public::error::{AppError, ErrorKind};

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
