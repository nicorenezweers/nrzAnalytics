//! Fehler der API: einheitlich als JSON `{"error": "...", "code": "..."}` mit passendem Status.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use nrzanalytics_shared::api::ApiError;

#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid", message)
    }

    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", "Bitte anmelden.")
    }

    pub fn forbidden() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Dafür fehlt die Berechtigung.",
        )
    }

    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "Nicht gefunden.")
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }

    pub fn rate_limited() -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Zu viele Anfragen – bitte kurz warten.",
        )
    }

    pub fn internal(message: impl std::fmt::Display) -> Self {
        tracing::error!("Interner Fehler: {message}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "Interner Fehler.")
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AppError {}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = ApiError {
            error: self.message,
            code: Some(self.code.to_string()),
        };
        (self.status, Json(body)).into_response()
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        match e {
            rusqlite::Error::QueryReturnedNoRows => Self::not_found(),
            rusqlite::Error::SqliteFailure(f, ref msg)
                if f.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                tracing::debug!("Constraint: {msg:?}");
                Self::conflict("Gibt es schon.")
            }
            other => Self::internal(other),
        }
    }
}

impl From<tokio::task::JoinError> for AppError {
    fn from(e: tokio::task::JoinError) -> Self {
        Self::internal(e)
    }
}
