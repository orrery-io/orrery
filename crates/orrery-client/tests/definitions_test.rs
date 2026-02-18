use orrery_client::OrreryClient;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn list_definitions_returns_items() {
    let server = MockServer::start().await;
    let body = serde_json::json!({
        "items": [{ "id": "proc:1", "version": 1, "created_at": "2026-02-20T00:00:00Z", "running_count": 0, "completed_count": 0, "failed_count": 0 }]
    });
    Mock::given(method("GET"))
        .and(path("/v1/process-definitions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let client = OrreryClient::new(server.uri());
    let result = client.list_definitions().await.unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].id, "proc:1");
}

#[tokio::test]
async fn deploy_definition_sends_xml() {
    let server = MockServer::start().await;
    let body = serde_json::json!({ "id": "proc:1", "version": 1, "created_at": "2026-02-20T00:00:00Z", "running_count": 0, "completed_count": 0, "failed_count": 0 });
    Mock::given(method("POST"))
        .and(path("/v1/process-definitions"))
        .respond_with(ResponseTemplate::new(201).set_body_json(&body))
        .mount(&server)
        .await;

    let client = OrreryClient::new(server.uri());
    let result = client
        .deploy_definition("<definitions/>".to_string())
        .await
        .unwrap();
    assert_eq!(result.id, "proc:1");
}

#[tokio::test]
async fn get_definition_404_returns_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/process-definitions/missing"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .mount(&server)
        .await;

    let client = OrreryClient::new(server.uri());
    let result = client.get_definition("missing").await;
    assert!(matches!(
        result,
        Err(orrery_client::ClientError::Http { status: 404, .. })
    ));
}
