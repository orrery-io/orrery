use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicSubscription {
    pub topic: String,
    #[serde(default)]
    pub process_definition_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchAndLockRequest {
    pub worker_id: String,
    pub subscriptions: Vec<TopicSubscription>,
    #[serde(default = "default_max_tasks")]
    pub max_tasks: u32,
    #[serde(default = "default_lock_duration_ms")]
    pub lock_duration_ms: u64,
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: u64,
}

fn default_max_tasks() -> u32 {
    1
}
fn default_lock_duration_ms() -> u64 {
    30_000
}
fn default_request_timeout_ms() -> u64 {
    20_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalTaskResponse {
    pub id: String,
    pub topic: String,
    pub process_instance_id: String,
    pub process_definition_id: String,
    pub element_id: String,
    pub variables: serde_json::Value,
    pub worker_id: String,
    pub locked_until: DateTime<Utc>,
    pub retry_count: i32,
    pub max_retries: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteExternalTaskRequest {
    pub worker_id: String,
    #[serde(default)]
    pub variables: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailExternalTaskRequest {
    pub worker_id: String,
    pub error_message: String,
    #[serde(default)]
    pub retries: i32,
    #[serde(default = "default_retry_timeout")]
    pub retry_timeout_ms: u64,
}

fn default_retry_timeout() -> u64 {
    0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendLockRequest {
    pub worker_id: String,
    pub new_duration_ms: u64,
}
