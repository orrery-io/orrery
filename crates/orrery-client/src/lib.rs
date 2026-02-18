use orrery_types::{
    BroadcastSignalRequest, BroadcastSignalResponse, ClaimTaskRequest, CompleteExternalTaskRequest,
    CompleteTaskRequest, ExtendLockRequest, ExternalTaskResponse, FailExternalTaskRequest,
    FailTaskRequest, FetchAndLockRequest, HistoryEntryResponse, ListDefinitionsResponse,
    OverviewMetrics, PaginatedInstancesResponse, ProcessDefinitionResponse,
    ProcessDefinitionVersionsResponse, ProcessInstanceResponse, SendMessageRequest,
    SendMessageResponse, StartInstanceRequest, TaskResponse, TimerResponse, UpdateTimerRequest,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("JSON decode error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, ClientError>;

/// HTTP client for the Orrery BPMN workflow engine REST API.
pub struct OrreryClient {
    base_url: String,
    http: reqwest::Client,
}

impl OrreryClient {
    /// Create a new client.
    ///
    /// `base_url` should be the server root, e.g. `"http://localhost:8080"`.
    /// Do not include `/v1` — the client adds it automatically.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/v1{}", self.base_url, path)
    }

    async fn check<T: serde::de::DeserializeOwned>(&self, resp: reqwest::Response) -> Result<T> {
        let status = resp.status().as_u16();
        if resp.status().is_success() {
            Ok(resp.json::<T>().await?)
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(ClientError::Http { status, body })
        }
    }

    // ── Definitions ───────────────────────────────────────────────────────────

    /// List all deployed process definitions.
    pub async fn list_definitions(&self) -> Result<ListDefinitionsResponse> {
        let resp = self
            .http
            .get(self.url("/process-definitions"))
            .send()
            .await?;
        self.check(resp).await
    }

    /// Deploy a new process definition from BPMN XML.
    pub async fn deploy_definition(&self, bpmn_xml: String) -> Result<ProcessDefinitionResponse> {
        let resp = self
            .http
            .post(self.url("/process-definitions"))
            .header("Content-Type", "text/xml")
            .body(bpmn_xml)
            .send()
            .await?;
        self.check(resp).await
    }

    /// Get a single process definition by ID.
    pub async fn get_definition(&self, id: &str) -> Result<ProcessDefinitionResponse> {
        let resp = self
            .http
            .get(self.url(&format!("/process-definitions/{id}")))
            .send()
            .await?;
        self.check(resp).await
    }

    /// List all deployed versions for a process definition, newest first.
    pub async fn list_definition_versions(
        &self,
        id: &str,
    ) -> Result<ProcessDefinitionVersionsResponse> {
        let resp = self
            .http
            .get(self.url(&format!("/process-definitions/{id}/versions")))
            .send()
            .await?;
        self.check(resp).await
    }

    // ── Process Instances ─────────────────────────────────────────────────────

    /// List process instances. Supports version and pagination filters.
    pub async fn list_instances(
        &self,
        definition_id: Option<&str>,
        state: Option<&str>,
        version: Option<i32>,
        page: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<PaginatedInstancesResponse> {
        let mut params: Vec<String> = Vec::new();
        if let Some(d) = definition_id {
            params.push(format!("definition_id={d}"));
        }
        if let Some(s) = state {
            params.push(format!("state={s}"));
        }
        if let Some(v) = version {
            params.push(format!("version={v}"));
        }
        if let Some(p) = page {
            params.push(format!("page={p}"));
        }
        if let Some(ps) = page_size {
            params.push(format!("page_size={ps}"));
        }
        let url = if params.is_empty() {
            self.url("/process-instances")
        } else {
            format!("{}?{}", self.url("/process-instances"), params.join("&"))
        };
        let resp = self.http.get(url).send().await?;
        self.check(resp).await
    }

    /// Start a new process instance.
    pub async fn start_instance(
        &self,
        req: StartInstanceRequest,
    ) -> Result<ProcessInstanceResponse> {
        let resp = self
            .http
            .post(self.url("/process-instances"))
            .json(&req)
            .send()
            .await?;
        self.check(resp).await
    }

    /// Get a single process instance by ID.
    pub async fn get_instance(&self, id: &str) -> Result<ProcessInstanceResponse> {
        let resp = self
            .http
            .get(self.url(&format!("/process-instances/{id}")))
            .send()
            .await?;
        self.check(resp).await
    }

    /// Merge variable updates into a process instance (non-destructive — unmentioned keys are preserved).
    pub async fn update_instance_variables(
        &self,
        id: &str,
        variables: serde_json::Value,
    ) -> Result<ProcessInstanceResponse> {
        let body = serde_json::json!({ "variables": variables });
        let resp = self
            .http
            .put(self.url(&format!("/process-instances/{id}/variables")))
            .json(&body)
            .send()
            .await?;
        self.check(resp).await
    }

    /// Cancel a running process instance.
    pub async fn cancel_instance(&self, id: &str) -> Result<ProcessInstanceResponse> {
        let resp = self
            .http
            .post(self.url(&format!("/process-instances/{id}/cancel")))
            .send()
            .await?;
        self.check(resp).await
    }

    /// Retry a failed process instance.
    /// Re-creates side effects (tasks, timers, subscriptions) for the active elements.
    pub async fn retry_instance(&self, id: &str) -> Result<ProcessInstanceResponse> {
        let resp = self
            .http
            .post(self.url(&format!("/process-instances/{id}/retry")))
            .send()
            .await?;
        self.check(resp).await
    }

    /// Get execution history for a process instance.
    /// Pass `level` as `Some("full")` to include gateway events, or `None`/`Some("activity")` for the default.
    pub async fn get_instance_history(
        &self,
        id: &str,
        level: Option<&str>,
    ) -> Result<Vec<HistoryEntryResponse>> {
        let mut url = self.url(&format!("/process-instances/{id}/history"));
        if let Some(lvl) = level {
            url = format!("{url}?level={lvl}");
        }
        let resp = self.http.get(url).send().await?;
        self.check(resp).await
    }

    // ── Tasks ─────────────────────────────────────────────────────────────────

    /// List tasks. Pass `state` and/or `instance_id` to filter.
    pub async fn list_tasks(
        &self,
        state: Option<&str>,
        instance_id: Option<&str>,
    ) -> Result<Vec<TaskResponse>> {
        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(s) = state {
            params.push(("state", s));
        }
        if let Some(i) = instance_id {
            params.push(("instance_id", i));
        }
        let url = if params.is_empty() {
            self.url("/tasks")
        } else {
            format!(
                "{}?{}",
                self.url("/tasks"),
                params
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("&")
            )
        };
        let resp = self.http.get(url).send().await?;
        self.check(resp).await
    }

    /// Get a single task by ID.
    pub async fn get_task(&self, id: &str) -> Result<TaskResponse> {
        let resp = self
            .http
            .get(self.url(&format!("/tasks/{id}")))
            .send()
            .await?;
        self.check(resp).await
    }

    /// Claim a task for processing.
    pub async fn claim_task(&self, id: &str, req: ClaimTaskRequest) -> Result<TaskResponse> {
        let resp = self
            .http
            .post(self.url(&format!("/tasks/{id}/claim")))
            .json(&req)
            .send()
            .await?;
        self.check(resp).await
    }

    /// Complete a task, advancing the process instance.
    pub async fn complete_task(&self, id: &str, req: CompleteTaskRequest) -> Result<TaskResponse> {
        let resp = self
            .http
            .post(self.url(&format!("/tasks/{id}/complete")))
            .json(&req)
            .send()
            .await?;
        self.check(resp).await
    }

    /// Fail a task, triggering retry or error handling.
    pub async fn fail_task(&self, id: &str, req: FailTaskRequest) -> Result<TaskResponse> {
        let resp = self
            .http
            .post(self.url(&format!("/tasks/{id}/fail")))
            .json(&req)
            .send()
            .await?;
        self.check(resp).await
    }

    /// Reset a failed task back to CREATED for another worker attempt.
    /// Increments retry_count by 1 (operator override, bypasses max_retries).
    pub async fn retry_task(&self, id: &str) -> Result<TaskResponse> {
        let resp = self
            .http
            .post(self.url(&format!("/tasks/{id}/retry")))
            .send()
            .await?;
        self.check(resp).await
    }

    // ── External Tasks ────────────────────────────────────────────────────────

    /// Fetch and lock external tasks (long-poll).
    pub async fn fetch_and_lock(
        &self,
        req: FetchAndLockRequest,
    ) -> Result<Vec<ExternalTaskResponse>> {
        let resp = self
            .http
            .post(self.url("/external-tasks/fetch-and-lock"))
            .json(&req)
            .timeout(std::time::Duration::from_millis(
                req.request_timeout_ms + 5_000,
            ))
            .send()
            .await?;
        self.check(resp).await
    }

    /// Complete an external task.
    pub async fn complete_external_task(
        &self,
        id: &str,
        req: CompleteExternalTaskRequest,
    ) -> Result<serde_json::Value> {
        let resp = self
            .http
            .post(self.url(&format!("/external-tasks/{id}/complete")))
            .json(&req)
            .send()
            .await?;
        self.check(resp).await
    }

    /// Report failure on an external task.
    pub async fn fail_external_task(
        &self,
        id: &str,
        req: FailExternalTaskRequest,
    ) -> Result<serde_json::Value> {
        let resp = self
            .http
            .post(self.url(&format!("/external-tasks/{id}/failure")))
            .json(&req)
            .send()
            .await?;
        self.check(resp).await
    }

    /// Extend the lock on an external task (heartbeat).
    pub async fn extend_lock(&self, id: &str, req: ExtendLockRequest) -> Result<serde_json::Value> {
        let resp = self
            .http
            .post(self.url(&format!("/external-tasks/{id}/extend-lock")))
            .json(&req)
            .send()
            .await?;
        self.check(resp).await
    }

    // ── Events ────────────────────────────────────────────────────────────────

    /// Send a message to a waiting process instance.
    pub async fn send_message(&self, req: SendMessageRequest) -> Result<SendMessageResponse> {
        let resp = self
            .http
            .post(self.url("/messages"))
            .json(&req)
            .send()
            .await?;
        self.check(resp).await
    }

    /// Broadcast a signal to all waiting process instances.
    pub async fn broadcast_signal(
        &self,
        signal_name: &str,
        req: BroadcastSignalRequest,
    ) -> Result<BroadcastSignalResponse> {
        let resp = self
            .http
            .post(self.url(&format!("/signals/{signal_name}")))
            .json(&req)
            .send()
            .await?;
        self.check(resp).await
    }

    // ── Metrics ───────────────────────────────────────────────────────────────────

    /// Fetch aggregate counts for the dashboard overview.
    pub async fn get_overview_metrics(&self) -> Result<OverviewMetrics> {
        let resp = self.http.get(self.url("/metrics/overview")).send().await?;
        self.check(resp).await
    }

    // ── Timers ────────────────────────────────────────────────────────────────

    /// List all scheduled timers for a process instance.
    pub async fn get_instance_timers(&self, instance_id: &str) -> Result<Vec<TimerResponse>> {
        let resp = self
            .http
            .get(self.url(&format!("/process-instances/{instance_id}/timers")))
            .send()
            .await?;
        self.check(resp).await
    }

    /// Immediately fire a pending timer, advancing the process instance.
    pub async fn fast_forward_timer(&self, instance_id: &str, timer_id: &str) -> Result<()> {
        let resp = self
            .http
            .post(self.url(&format!(
                "/process-instances/{instance_id}/timers/{timer_id}/fast-forward"
            )))
            .send()
            .await?;
        let status = resp.status().as_u16();
        if resp.status().is_success() {
            Ok(())
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(ClientError::Http { status, body })
        }
    }

    /// Reschedule a pending timer to a new due_at time.
    pub async fn update_timer(
        &self,
        instance_id: &str,
        timer_id: &str,
        req: UpdateTimerRequest,
    ) -> Result<TimerResponse> {
        let resp = self
            .http
            .put(self.url(&format!(
                "/process-instances/{instance_id}/timers/{timer_id}"
            )))
            .json(&req)
            .send()
            .await?;
        self.check(resp).await
    }
}

#[cfg(test)]
mod tests {
    use super::{ClientError, OrreryClient};

    #[test]
    fn error_display_http() {
        let e = ClientError::Http {
            status: 404,
            body: "not found".into(),
        };
        assert!(e.to_string().contains("404"));
    }

    #[test]
    fn client_builds_with_base_url() {
        let _client = OrreryClient::new("http://localhost:8080");
        // just checks it doesn't panic
    }

    #[tokio::test]
    async fn fetch_and_lock_sends_correct_request() {
        use orrery_types::{FetchAndLockRequest, TopicSubscription};
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/external-tasks/fetch-and-lock"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;

        let client = OrreryClient::new(server.uri());
        let result = client
            .fetch_and_lock(FetchAndLockRequest {
                worker_id: "w1".into(),
                subscriptions: vec![TopicSubscription {
                    topic: "payments".into(),
                    process_definition_ids: vec![],
                }],
                max_tasks: 1,
                lock_duration_ms: 30_000,
                request_timeout_ms: 1_000,
            })
            .await
            .unwrap();
        assert!(result.is_empty());
    }
}
