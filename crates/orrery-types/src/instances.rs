use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StartInstanceRequest {
    pub process_definition_id: String,
    pub business_key: Option<String>,
    #[serde(default)]
    pub variables: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInstanceResponse {
    pub id: String,
    pub process_definition_id: String,
    pub process_definition_version: i32,
    pub business_key: Option<String>,
    pub state: String,
    pub variables: Value,
    pub active_element_ids: Value,
    pub created_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
    /// For FAILED instances: the element_id of the failed task (populated server-side).
    #[serde(default)]
    pub failed_at_element_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedInstancesResponse {
    pub items: Vec<ProcessInstanceResponse>,
    pub total: i64,
    pub page: u32,
    pub page_size: u32,
    pub total_pages: u32,
}
