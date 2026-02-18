use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SendMessageRequest {
    pub message_ref: String,
    #[serde(default)]
    pub correlation_data: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageResponse {
    pub instance_id: String,
    pub element_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BroadcastSignalRequest {
    #[serde(default)]
    pub variables: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcastSignalResponse {
    pub woken_count: i64,
}
