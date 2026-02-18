use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntryResponse {
    pub id: i64,
    pub element_id: String,
    pub element_name: Option<String>,
    pub element_type: String,
    pub event_type: String,
    pub variables_snapshot: Value,
    pub ordering: i32,
    pub occurred_at: DateTime<Utc>,
}
