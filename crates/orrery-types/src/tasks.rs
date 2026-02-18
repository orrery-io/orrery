use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResponse {
    pub id: String,
    pub process_instance_id: String,
    pub process_definition_id: String,
    pub element_id: String,
    pub element_type: String,
    pub state: String,
    pub claimed_by: Option<String>,
    pub variables: Value,
    pub created_at: DateTime<Utc>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub retry_count: i32,
    #[serde(default)]
    pub max_retries: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClaimTaskRequest {
    pub claimed_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CompleteTaskRequest {
    #[serde(default)]
    pub variables: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FailTaskRequest {
    pub reason: String,
}
