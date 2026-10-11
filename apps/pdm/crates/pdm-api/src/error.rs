use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use pdm_core::PdmError;

/// Error wrapper for axum handlers. Carries the domain error and maps it to an HTTP status.
#[derive(Debug)]
pub struct AppError(pub PdmError);

impl From<PdmError> for AppError {
    fn from(e: PdmError) -> Self {
        Self(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.0.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = serde_json::json!({
            "error": {
                "code": self.0.code(),
                "message": self.0.to_string(),
            }
        });
        (status, axum::Json(body)).into_response()
    }
}
