use axum::http::StatusCode;
use axum_test::TestServer;
use serde_json::{json, Value};
use sqlx::PgPool;
use testcontainers_modules::{postgres::Postgres, testcontainers::runners::AsyncRunner};

async fn setup() -> (TestServer, impl Drop) {
    let container = Postgres::default().start().await.unwrap();
    let host_port = container.get_host_port_ipv4(5432).await.unwrap();
    let db_url = format!("postgres://postgres:postgres@127.0.0.1:{host_port}/postgres");

    let pool = PgPool::connect(&db_url).await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    let app = orrery_server::build_app(pool);
    let server = TestServer::new(app).unwrap();

    (server, container)
}

const EBG_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL"
                  xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
                  targetNamespace="http://test">
  <bpmn:message id="msg1" name="OrderReceived"/>
  <bpmn:process id="ebg-test" isExecutable="true">
    <bpmn:startEvent id="start"><bpmn:outgoing>f1</bpmn:outgoing></bpmn:startEvent>
    <bpmn:eventBasedGateway id="ebg">
      <bpmn:incoming>f1</bpmn:incoming>
      <bpmn:outgoing>f2</bpmn:outgoing>
      <bpmn:outgoing>f3</bpmn:outgoing>
    </bpmn:eventBasedGateway>
    <bpmn:intermediateCatchEvent id="msg_catch">
      <bpmn:incoming>f2</bpmn:incoming><bpmn:outgoing>f4</bpmn:outgoing>
      <bpmn:messageEventDefinition messageRef="msg1"/>
    </bpmn:intermediateCatchEvent>
    <bpmn:intermediateCatchEvent id="timer_catch">
      <bpmn:incoming>f3</bpmn:incoming><bpmn:outgoing>f5</bpmn:outgoing>
      <bpmn:timerEventDefinition><bpmn:timeDuration>PT1H</bpmn:timeDuration></bpmn:timerEventDefinition>
    </bpmn:intermediateCatchEvent>
    <bpmn:serviceTask id="task_msg"><bpmn:incoming>f4</bpmn:incoming><bpmn:outgoing>f6</bpmn:outgoing></bpmn:serviceTask>
    <bpmn:serviceTask id="task_timer"><bpmn:incoming>f5</bpmn:incoming><bpmn:outgoing>f7</bpmn:outgoing></bpmn:serviceTask>
    <bpmn:endEvent id="end_msg"><bpmn:incoming>f6</bpmn:incoming></bpmn:endEvent>
    <bpmn:endEvent id="end_timer"><bpmn:incoming>f7</bpmn:incoming></bpmn:endEvent>
    <bpmn:sequenceFlow id="f1" sourceRef="start" targetRef="ebg"/>
    <bpmn:sequenceFlow id="f2" sourceRef="ebg" targetRef="msg_catch"/>
    <bpmn:sequenceFlow id="f3" sourceRef="ebg" targetRef="timer_catch"/>
    <bpmn:sequenceFlow id="f4" sourceRef="msg_catch" targetRef="task_msg"/>
    <bpmn:sequenceFlow id="f5" sourceRef="timer_catch" targetRef="task_timer"/>
    <bpmn:sequenceFlow id="f6" sourceRef="task_msg" targetRef="end_msg"/>
    <bpmn:sequenceFlow id="f7" sourceRef="task_timer" targetRef="end_timer"/>
  </bpmn:process>
</bpmn:definitions>"#;

/// Helper: deploy EBG BPMN and start an instance, returning (server, instance_id, _container)
async fn deploy_and_start() -> (TestServer, String, impl Drop) {
    let (server, container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(EBG_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "ebg-test" }))
        .await;
    response.assert_status(StatusCode::CREATED);
    let body: Value = response.json();
    let instance_id = body["id"].as_str().unwrap().to_string();

    (server, instance_id, container)
}

#[tokio::test]
async fn test_ebg_creates_both_subscriptions() {
    let (server, instance_id, _container) = deploy_and_start().await;

    // Timer subscription should exist
    let timers: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/timers"))
        .await
        .json();
    let unfired: Vec<&Value> = timers
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["fired"] == false)
        .collect();
    assert_eq!(unfired.len(), 1, "Should have 1 pending timer subscription");
    assert_eq!(unfired[0]["element_id"], "timer_catch");

    // Instance should be in RUNNING state (mixed wait states: message + timer)
    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "RUNNING");
}

#[tokio::test]
async fn test_ebg_message_wins_cancels_timer() {
    let (server, instance_id, _container) = deploy_and_start().await;

    // Verify timer exists before message
    let timers: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/timers"))
        .await
        .json();
    let unfired: Vec<&Value> = timers
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["fired"] == false)
        .collect();
    assert_eq!(
        unfired.len(),
        1,
        "Timer subscription should exist before message"
    );

    // Send the message — message branch wins
    let msg_resp = server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "OrderReceived",
            "process_instance_id": instance_id,
            "variables": {}
        }))
        .await;
    msg_resp.assert_status_ok();

    // Timer subscription should be cancelled (deleted)
    let timers_after: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/timers"))
        .await
        .json();
    let unfired_after: Vec<&Value> = timers_after
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["fired"] == false)
        .collect();
    assert_eq!(
        unfired_after.len(),
        0,
        "Timer should be cancelled after message wins"
    );

    // Instance should now be at task_msg (waiting for service task)
    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    let active: Vec<&Value> = tasks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["state"] == "CREATED")
        .collect();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0]["element_id"], "task_msg");

    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_TASK");
}

#[tokio::test]
async fn test_ebg_timer_wins_cancels_message_subscription() {
    let (server, instance_id, _container) = deploy_and_start().await;

    // Get the timer ID for fast-forward
    let timers: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/timers"))
        .await
        .json();
    let timer_id = timers[0]["id"].as_str().unwrap().to_string();

    // Fast-forward the timer — timer branch wins
    server
        .post(&format!(
            "/v1/process-instances/{instance_id}/timers/{timer_id}/fast-forward"
        ))
        .await
        .assert_status(StatusCode::NO_CONTENT);

    // Instance should now be at task_timer
    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    let active: Vec<&Value> = tasks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["state"] == "CREATED")
        .collect();
    assert_eq!(active.len(), 1, "Only timer branch task should exist");
    assert_eq!(active[0]["element_id"], "task_timer");

    // Message should no longer match (subscription was cancelled)
    let msg_resp = server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "OrderReceived",
            "process_instance_id": instance_id,
            "variables": {}
        }))
        .await;
    // Should be 404 because the message subscription was deleted
    msg_resp.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_ebg_message_wins_completes_to_end() {
    let (server, instance_id, _container) = deploy_and_start().await;

    // Send message to advance to task_msg
    server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "OrderReceived",
            "process_instance_id": instance_id,
            "variables": {}
        }))
        .await
        .assert_status_ok();

    // Complete task_msg
    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    let task = tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["state"] == "CREATED")
        .unwrap();
    let task_id = task["id"].as_str().unwrap().to_string();

    server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({ "claimed_by": "worker-1" }))
        .await
        .assert_status_ok();

    server
        .post(&format!("/v1/tasks/{task_id}/complete"))
        .json(&json!({ "variables": {} }))
        .await
        .assert_status_ok();

    // Instance should be COMPLETED
    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "COMPLETED");
}
