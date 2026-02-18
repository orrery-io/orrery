use orrery_client::OrreryClient;
use orrery_types::OverviewMetrics;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_get_overview_metrics() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/metrics/overview"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "running_instances": 3,
            "waiting_instances": 2,
            "completed_instances": 10,
            "failed_instances": 1,
            "pending_tasks": 5,
            "claimed_tasks": 1,
        })))
        .mount(&server)
        .await;

    let client = OrreryClient::new(server.uri());
    let metrics: OverviewMetrics = client.get_overview_metrics().await.unwrap();
    assert_eq!(metrics.running_instances, 3);
    assert_eq!(metrics.pending_tasks, 5);
}
