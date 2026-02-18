use orrery_client::OrreryClient;
use orrery_types::StartInstanceRequest;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn instance_json(id: &str, state: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "process_definition_id": "proc",
        "process_definition_version": 1,
        "state": state,
        "variables": {},
        "active_element_ids": [],
        "created_at": "2026-02-20T00:00:00Z",
        "ended_at": null,
    })
}

#[tokio::test]
async fn start_instance_returns_201() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/process-instances"))
        .respond_with(ResponseTemplate::new(201).set_body_json(instance_json("inst-1", "RUNNING")))
        .mount(&server)
        .await;

    let client = OrreryClient::new(server.uri());
    let req = StartInstanceRequest {
        process_definition_id: "proc:1".into(),
        ..Default::default()
    };
    let result = client.start_instance(req).await.unwrap();
    assert_eq!(result.id, "inst-1");
}

#[tokio::test]
async fn list_instances_returns_vec() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/process-instances"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [instance_json("inst-1", "RUNNING")],
            "total": 1,
            "page": 1,
            "page_size": 20,
            "total_pages": 1,
        })))
        .mount(&server)
        .await;

    let client = OrreryClient::new(server.uri());
    let result = client
        .list_instances(None, None, None, None, None)
        .await
        .unwrap();
    assert_eq!(result.items.len(), 1);
}

#[tokio::test]
async fn get_instance_404_returns_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/process-instances/missing"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .mount(&server)
        .await;

    let client = OrreryClient::new(server.uri());
    let result = client.get_instance("missing").await;
    assert!(matches!(
        result,
        Err(orrery_client::ClientError::Http { status: 404, .. })
    ));
}

#[tokio::test]
async fn cancel_instance_returns_instance() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/process-instances/inst-1/cancel"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(instance_json("inst-1", "CANCELLED")),
        )
        .mount(&server)
        .await;

    let client = OrreryClient::new(server.uri());
    let result = client.cancel_instance("inst-1").await.unwrap();
    assert_eq!(result.state, "CANCELLED");
}

#[tokio::test]
async fn get_history_returns_entries() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/process-instances/inst-1/history"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;

    let client = OrreryClient::new(server.uri());
    let result = client.get_instance_history("inst-1", None).await.unwrap();
    assert!(result.is_empty());
}
