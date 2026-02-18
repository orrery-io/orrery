use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use utoipa::ToSchema;

/// OpenAPI schema for error responses.
#[derive(Serialize, ToSchema)]
pub struct ErrorResponse {
    /// Machine-readable error code (e.g. `INSTANCE_NOT_FOUND`)
    pub code: String,
    /// Human-readable error message
    pub message: String,
}

/// Structured API error with machine-readable code and human-readable message.
///
/// Response format:
/// ```json
/// { "code": "INSTANCE_NOT_FOUND", "message": "Instance 'abc-123' not found" }
/// ```
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let body = serde_json::json!({
            "code": self.code,
            "message": self.message,
        });
        (self.status, Json(body)).into_response()
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

/// Backward-compat: converts legacy `(StatusCode, String)` tuples into `ApiError`.
/// All converted errors get `INTERNAL_ERROR` as the code.
impl From<(StatusCode, String)> for ApiError {
    fn from((status, message): (StatusCode, String)) -> Self {
        ApiError {
            status,
            code: codes::INTERNAL_ERROR,
            message,
        }
    }
}

impl ApiError {
    pub fn not_found(code: &'static str, message: impl Into<String>) -> Self {
        ApiError {
            status: StatusCode::NOT_FOUND,
            code,
            message: message.into(),
        }
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        ApiError {
            status: StatusCode::CONFLICT,
            code,
            message: message.into(),
        }
    }

    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        ApiError {
            status: StatusCode::BAD_REQUEST,
            code,
            message: message.into(),
        }
    }

    pub fn unprocessable(code: &'static str, message: impl Into<String>) -> Self {
        ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: codes::INTERNAL_ERROR,
            message: message.into(),
        }
    }
}

/// Machine-readable error code constants.
pub mod codes {
    // ── Not Found ──────────────────────────────────────────────────────
    pub const DEFINITION_NOT_FOUND: &str = "DEFINITION_NOT_FOUND";
    pub const INSTANCE_NOT_FOUND: &str = "INSTANCE_NOT_FOUND";
    pub const TASK_NOT_FOUND: &str = "TASK_NOT_FOUND";
    pub const TIMER_NOT_FOUND: &str = "TIMER_NOT_FOUND";
    pub const MESSAGE_NO_MATCH: &str = "MESSAGE_NO_MATCH";

    // ── Conflict ───────────────────────────────────────────────────────
    pub const INSTANCE_TERMINAL: &str = "INSTANCE_TERMINAL";
    pub const TASK_STATE_CONFLICT: &str = "TASK_STATE_CONFLICT";
    pub const TASK_LOCK_EXPIRED: &str = "TASK_LOCK_EXPIRED";
    pub const TASK_WRONG_OWNER: &str = "TASK_WRONG_OWNER";
    pub const CORRELATION_AMBIGUOUS: &str = "CORRELATION_AMBIGUOUS";

    // ── Validation ─────────────────────────────────────────────────────
    pub const INVALID_BPMN: &str = "INVALID_BPMN";
    pub const ENGINE_REJECTED: &str = "ENGINE_REJECTED";
    pub const INVALID_VARIABLES: &str = "INVALID_VARIABLES";
    pub const INVALID_TIMER_EXPR: &str = "INVALID_TIMER_EXPR";

    // ── Internal ───────────────────────────────────────────────────────
    pub const INTERNAL_ERROR: &str = "INTERNAL_ERROR";
}
