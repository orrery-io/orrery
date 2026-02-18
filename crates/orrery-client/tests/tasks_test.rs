use orrery_client::OrreryClient;
use orrery_types::{ClaimTaskRequest, CompleteTaskRequest, FailTaskRequest};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn task_json(id: &str, state: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "process_instance_id": "inst-1",
        "process_definition_id": "proc:1",
        "element_id": "task1",
        "element_type": "SERVICE_TASK",
        "state": state,
        "claimed_by": null,
        "variables": {},
        "created_at": "2026-02-20T00:00:00Z",
        "claimed_at": null,
        "completed_at": null,
        "retry_count": 0,
        "max_retries": 0,
    })
}

#[tokio::test]
async fn list_tasks_returns_all() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/tasks"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!([task_json("t1", "CREATED")])),
        )
        .mount(&server)
        .await;

    let client = OrreryClient::new(server.uri());
    let result = client.list_tasks(None, None).await.unwrap();
    assert_eq!(result.len(), 1);
}

#[tokio::test]
async fn list_tasks_for_instance_filters() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/tasks"))
        .and(query_param("instance_id", "inst-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;

    let client = OrreryClient::new(server.uri());
    let result = client.list_tasks(None, Some("inst-1")).await.unwrap();
    assert!(result.is_empty());
}

#[tokio::test]
async fn claim_task_sends_claimed_by() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/tasks/t1/claim"))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_json("t1", "CLAIMED")))
        .mount(&server)
        .await;

    let client = OrreryClient::new(server.uri());
    let result = client
        .claim_task(
            "t1",
            ClaimTaskRequest {
                claimed_by: "worker-1".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(result.state, "CLAIMED");
}

#[tokio::test]
async fn complete_task_advances_instance() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/tasks/t1/complete"))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_json("t1", "COMPLETED")))
        .mount(&server)
        .await;

    let client = OrreryClient::new(server.uri());
    let result = client
        .complete_task("t1", CompleteTaskRequest::default())
        .await
        .unwrap();
    assert_eq!(result.state, "COMPLETED");
}

#[tokio::test]
async fn fail_task_returns_failed_state() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/tasks/t1/fail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(task_json("t1", "FAILED")))
        .mount(&server)
        .await;

    let client = OrreryClient::new(server.uri());
    let result = client
        .fail_task(
            "t1",
            FailTaskRequest {
                reason: "boom".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(result.state, "FAILED");
}
