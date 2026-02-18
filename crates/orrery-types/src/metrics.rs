use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverviewMetrics {
    pub running_instances: i64,
    pub waiting_instances: i64,
    pub completed_instances: i64,
    pub failed_instances: i64,
    pub pending_tasks: i64,
    pub claimed_tasks: i64,
}
