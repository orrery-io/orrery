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

// Inclusive gateway: f2 has condition (amount > 100), f3 is unconditional.
// When amount > 100: both task_a and task_b activate.
// When amount <= 100: only task_b activates.
const INCLUSIVE_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="ig">
  <process id="inclusive-test" isExecutable="true">
    <startEvent id="start"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start" targetRef="fork"/>
    <inclusiveGateway id="fork">
      <incoming>f1</incoming>
      <outgoing>f2</outgoing>
      <outgoing>f3</outgoing>
    </inclusiveGateway>
    <sequenceFlow id="f2" sourceRef="fork" targetRef="task_a">
      <conditionExpression>amount &gt; 100</conditionExpression>
    </sequenceFlow>
    <sequenceFlow id="f3" sourceRef="fork" targetRef="task_b"/>
    <serviceTask id="task_a" name="Task A">
      <incoming>f2</incoming>
      <outgoing>f4</outgoing>
    </serviceTask>
    <serviceTask id="task_b" name="Task B">
      <incoming>f3</incoming>
      <outgoing>f5</outgoing>
    </serviceTask>
    <sequenceFlow id="f4" sourceRef="task_a" targetRef="join"/>
    <sequenceFlow id="f5" sourceRef="task_b" targetRef="join"/>
    <inclusiveGateway id="join">
      <incoming>f4</incoming>
      <incoming>f5</incoming>
      <outgoing>f6</outgoing>
    </inclusiveGateway>
    <sequenceFlow id="f6" sourceRef="join" targetRef="end"/>
    <endEvent id="end"><incoming>f6</incoming></endEvent>
  </process>
</definitions>"#;

#[tokio::test]
async fn test_inclusive_all_conditions_true_creates_two_tasks() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(INCLUSIVE_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "inclusive-test",
            "variables": { "amount": 200 }
        }))
        .await;
    response.assert_status(StatusCode::CREATED);
    let body: Value = response.json();
    let instance_id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["state"], "WAITING_FOR_TASK");

    // Both branches should be active
    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    let tasks_arr = tasks.as_array().unwrap();
    assert_eq!(
        tasks_arr.len(),
        2,
        "Expected 2 tasks when condition is true"
    );

    let element_ids: Vec<&str> = tasks_arr
        .iter()
        .map(|t| t["element_id"].as_str().unwrap())
        .collect();
    assert!(element_ids.contains(&"task_a"));
    assert!(element_ids.contains(&"task_b"));
}

#[tokio::test]
async fn test_inclusive_condition_false_creates_one_task() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(INCLUSIVE_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "inclusive-test",
            "variables": { "amount": 50 }
        }))
        .await;
    response.assert_status(StatusCode::CREATED);
    let body: Value = response.json();
    let instance_id = body["id"].as_str().unwrap().to_string();

    // Only unconditional branch (task_b) should be active
    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    let tasks_arr = tasks.as_array().unwrap();
    assert_eq!(
        tasks_arr.len(),
        1,
        "Expected 1 task when condition is false"
    );
    assert_eq!(tasks_arr[0]["element_id"], "task_b");
}

#[tokio::test]
async fn test_inclusive_partial_join_waits_for_correct_count() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(INCLUSIVE_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start with both branches active
    let response = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "inclusive-test",
            "variables": { "amount": 200 }
        }))
        .await;
    let body: Value = response.json();
    let instance_id = body["id"].as_str().unwrap().to_string();

    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    let task_a = tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["element_id"] == "task_a")
        .unwrap();
    let task_a_id = task_a["id"].as_str().unwrap().to_string();

    // Complete task_a — instance should still be active (waiting for task_b at join)
    server
        .post(&format!("/v1/tasks/{task_a_id}/claim"))
        .json(&json!({ "claimed_by": "worker-1" }))
        .await
        .assert_status_ok();
    server
        .post(&format!("/v1/tasks/{task_a_id}/complete"))
        .json(&json!({ "variables": {} }))
        .await
        .assert_status_ok();

    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(
        inst["state"], "WAITING_FOR_TASK",
        "Should wait for task_b at join"
    );
}

#[tokio::test]
async fn test_inclusive_both_complete_finishes_instance() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(INCLUSIVE_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "inclusive-test",
            "variables": { "amount": 200 }
        }))
        .await;
    let body: Value = response.json();
    let instance_id = body["id"].as_str().unwrap().to_string();

    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();

    for task in tasks.as_array().unwrap() {
        let task_id = task["id"].as_str().unwrap();
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
    }

    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "COMPLETED");
}

#[tokio::test]
async fn test_inclusive_single_branch_completes_at_join() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(INCLUSIVE_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Only one branch active (amount <= 100)
    let response = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "inclusive-test",
            "variables": { "amount": 50 }
        }))
        .await;
    let body: Value = response.json();
    let instance_id = body["id"].as_str().unwrap().to_string();

    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    assert_eq!(tasks.as_array().unwrap().len(), 1);
    let task_id = tasks[0]["id"].as_str().unwrap().to_string();

    // Complete the single task — join should fire immediately, instance completes
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

    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "COMPLETED");
}
