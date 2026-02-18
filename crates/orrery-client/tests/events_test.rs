use orrery_client::OrreryClient;
use orrery_types::{BroadcastSignalRequest, SendMessageRequest};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn send_message_returns_matched_instance() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "instance_id": "inst-1",
            "element_id": "msg-catch"
        })))
        .mount(&server)
        .await;

    let client = OrreryClient::new(server.uri());
    let req = SendMessageRequest {
        message_ref: "PaymentApproved".into(),
        ..Default::default()
    };
    let result = client.send_message(req).await.unwrap();
    assert_eq!(result.instance_id, "inst-1");
}

#[tokio::test]
async fn send_message_404_returns_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(404).set_body_string("no match"))
        .mount(&server)
        .await;

    let client = OrreryClient::new(server.uri());
    let req = SendMessageRequest {
        message_ref: "NoSuch".into(),
        ..Default::default()
    };
    let result = client.send_message(req).await;
    assert!(matches!(
        result,
        Err(orrery_client::ClientError::Http { status: 404, .. })
    ));
}

#[tokio::test]
async fn broadcast_signal_returns_count() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/signals/OrderCancelled"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "woken_count": 3
        })))
        .mount(&server)
        .await;

    let client = OrreryClient::new(server.uri());
    let req = BroadcastSignalRequest::default();
    let result = client
        .broadcast_signal("OrderCancelled", req)
        .await
        .unwrap();
    assert_eq!(result.woken_count, 3);
}
