use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimerResponse {
    pub id: String,
    pub element_id: String,
    /// Timer kind: "duration", "date", or "cycle"
    pub kind: String,
    /// The timer expression (ISO 8601 duration, date, or repeating interval)
    pub expression: Option<String>,
    pub due_at: DateTime<Utc>,
    pub fired: bool,
    pub fired_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateTimerRequest {
    /// New ISO 8601 expression (e.g. "PT10M", "2026-04-01T12:00:00Z", "R3/PT1H").
    /// Server re-evaluates due_at from this expression.
    pub expression: String,
}
