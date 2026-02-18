use orrery_client::OrreryClient;
use orrery_types::{
    HistoryEntryResponse, ListDefinitionsResponse, OverviewMetrics, PaginatedInstancesResponse,
    ProcessDefinitionResponse, ProcessDefinitionVersionsResponse, ProcessInstanceResponse,
    StartInstanceRequest, TaskResponse, TimerResponse, UpdateTimerRequest,
};

fn client() -> OrreryClient {
    let api_url = std::env::var("ORRERY_API_URL").unwrap_or("http://localhost:3000".to_string());
    OrreryClient::new(api_url)
}

// ── Definitions ───────────────────────────────────────────────────────────────

pub async fn deploy_definition(bpmn_xml: String) -> Result<ProcessDefinitionResponse, String> {
    client()
        .deploy_definition(bpmn_xml)
        .await
        .map_err(|e| e.to_string())
}

pub async fn list_definitions() -> Result<ListDefinitionsResponse, String> {
    client().list_definitions().await.map_err(|e| e.to_string())
}

pub async fn list_definition_versions(
    id: &str,
) -> Result<ProcessDefinitionVersionsResponse, String> {
    client()
        .list_definition_versions(id)
        .await
        .map_err(|e| e.to_string())
}

// ── Instances ─────────────────────────────────────────────────────────────────

pub async fn start_instance(req: StartInstanceRequest) -> Result<ProcessInstanceResponse, String> {
    client()
        .start_instance(req)
        .await
        .map_err(|e| e.to_string())
}

pub async fn list_instances(
    definition_id: Option<&str>,
    state: Option<&str>,
) -> Result<PaginatedInstancesResponse, String> {
    client()
        .list_instances(definition_id, state, None, None, None)
        .await
        .map_err(|e| e.to_string())
}

pub async fn list_instances_for_definition(
    definition_id: &str,
    state: Option<&str>,
    version: Option<i32>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<PaginatedInstancesResponse, String> {
    client()
        .list_instances(Some(definition_id), state, version, page, page_size)
        .await
        .map_err(|e| e.to_string())
}

pub async fn get_instance(id: &str) -> Result<ProcessInstanceResponse, String> {
    client().get_instance(id).await.map_err(|e| e.to_string())
}

pub async fn update_instance_variables(
    instance_id: &str,
    variables: serde_json::Value,
) -> Result<ProcessInstanceResponse, String> {
    client()
        .update_instance_variables(instance_id, variables)
        .await
        .map_err(|e| e.to_string())
}

pub async fn cancel_instance(id: &str) -> Result<ProcessInstanceResponse, String> {
    client()
        .cancel_instance(id)
        .await
        .map_err(|e| e.to_string())
}

pub async fn retry_instance(id: &str) -> Result<ProcessInstanceResponse, String> {
    client().retry_instance(id).await.map_err(|e| e.to_string())
}

pub async fn get_instance_history(
    id: &str,
    level: Option<&str>,
) -> Result<Vec<HistoryEntryResponse>, String> {
    client()
        .get_instance_history(id, level)
        .await
        .map_err(|e| e.to_string())
}

// ── Tasks ─────────────────────────────────────────────────────────────────────

pub async fn list_tasks(
    state: Option<&str>,
    instance_id: Option<&str>,
) -> Result<Vec<TaskResponse>, String> {
    client()
        .list_tasks(state, instance_id)
        .await
        .map_err(|e| e.to_string())
}

pub async fn list_tasks_for_instance(instance_id: &str) -> Result<Vec<TaskResponse>, String> {
    client()
        .list_tasks(None, Some(instance_id))
        .await
        .map_err(|e| e.to_string())
}

pub async fn retry_task(task_id: &str) -> Result<TaskResponse, String> {
    client()
        .retry_task(task_id)
        .await
        .map_err(|e| e.to_string())
}

// ── Timers ────────────────────────────────────────────────────────────────────

pub async fn get_instance_timers(instance_id: &str) -> Result<Vec<TimerResponse>, String> {
    client()
        .get_instance_timers(instance_id)
        .await
        .map_err(|e| e.to_string())
}

pub async fn fast_forward_timer(instance_id: &str, timer_id: &str) -> Result<(), String> {
    client()
        .fast_forward_timer(instance_id, timer_id)
        .await
        .map_err(|e| e.to_string())
}

pub async fn update_timer_expression(
    instance_id: &str,
    timer_id: &str,
    expression: String,
) -> Result<TimerResponse, String> {
    client()
        .update_timer(instance_id, timer_id, UpdateTimerRequest { expression })
        .await
        .map_err(|e| e.to_string())
}

// ── Metrics ──────────────────────────────────────────────────────────────────

pub async fn get_overview_metrics() -> Result<OverviewMetrics, String> {
    client()
        .get_overview_metrics()
        .await
        .map_err(|e| e.to_string())
}
