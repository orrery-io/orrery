use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessDefinitionResponse {
    pub id: String,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub running_count: i64,
    pub completed_count: i64,
    pub failed_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDefinitionsResponse {
    pub items: Vec<ProcessDefinitionResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessDefinitionVersionsResponse {
    /// All deployed versions of this definition, newest first (e.g. [3, 2, 1])
    pub versions: Vec<i32>,
    /// The highest version number
    pub latest: i32,
}
