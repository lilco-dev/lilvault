use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid or unsupported path")]
    BadPath,
    #[error("resource not found")]
    NotFound,
    #[error("bearer authentication required")]
    Unauthorized,
    #[error("credential has read-only access")]
    Forbidden,
    #[error("a create-only or matching-revision precondition is required")]
    PreconditionRequired,
    #[error("revision precondition did not match")]
    PreconditionFailed,
    #[error("invalid request")]
    BadRequest,
    #[error("file or request exceeds the supported limit")]
    TooLarge,
    #[error("vault is not safely available; a timed-out write may still complete")]
    Unavailable,
    #[error("request timed out; read the current revision before retrying a write")]
    Timeout,
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadPath => "invalid_path",
            Self::NotFound => "not_found",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::PreconditionRequired => "precondition_required",
            Self::PreconditionFailed => "precondition_failed",
            Self::BadRequest => "invalid_request",
            Self::TooLarge => "too_large",
            Self::Unavailable => "unavailable",
            Self::Timeout => "timeout",
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadPath | Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::PreconditionRequired => StatusCode::PRECONDITION_REQUIRED,
            Self::PreconditionFailed => StatusCode::PRECONDITION_FAILED,
            Self::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Timeout => StatusCode::GATEWAY_TIMEOUT,
        }
    }
}

#[derive(Serialize)]
struct ErrorDetails {
    code: &'static str,
    message: String,
}
#[derive(Serialize)]
struct ErrorResponse {
    error: ErrorDetails,
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let mut response = (
            self.status(),
            Json(ErrorResponse {
                error: ErrorDetails {
                    code: self.code(),
                    message: self.to_string(),
                },
            }),
        )
            .into_response();
        if matches!(self, Self::Unauthorized) {
            response
                .headers_mut()
                .insert("www-authenticate", "Bearer".parse().expect("static header"));
        }
        response
    }
}

pub(crate) fn io(error: std::io::Error) -> Error {
    match error.raw_os_error() {
        Some(2) => Error::NotFound,
        Some(20 | 40 | 18) => Error::BadPath, // ENOTDIR, ELOOP, EXDEV
        _ => Error::Unavailable,
    }
}
