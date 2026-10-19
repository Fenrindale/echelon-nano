use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
#[error("{code}")]
pub struct AppError {
    pub status: StatusCode,
    pub code: &'static str,
}

impl AppError {
    pub fn new(status: StatusCode, code: &'static str) -> Self {
        Self { status, code }
    }
    pub fn internal(code: &'static str) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, code)
    }
    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized")
    }
    pub fn forbidden(code: &'static str) -> Self {
        Self::new(StatusCode::FORBIDDEN, code)
    }
    pub fn bad(code: &'static str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({"error":{"code":self.code}}))).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
