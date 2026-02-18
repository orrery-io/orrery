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

const PARALLEL_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="par">
  <process id="parallel-test" isExecutable="true">
    <startEvent id="start"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start" targetRef="fork"/>
    <parallelGateway id="fork">
      <incoming>f1</incoming>
      <outgoing>f2</outgoing>
      <outgoing>f3</outgoing>
    </parallelGateway>
    <sequenceFlow id="f2" sourceRef="fork" targetRef="task_a"/>
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
    <parallelGateway id="join">
      <incoming>f4</incoming>
      <incoming>f5</incoming>
      <outgoing>f6</outgoing>
    </parallelGateway>
    <sequenceFlow id="f6" sourceRef="join" targetRef="end"/>
    <endEvent id="end"><incoming>f6</incoming></endEvent>
  </process>
</definitions>"#;

#[tokio::test]
async fn test_parallel_fork_creates_two_tasks() {
    let (server, _container) = setup().await;

    // Deploy
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(PARALLEL_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start instance
    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "parallel-test" }))
        .await;
    response.assert_status(StatusCode::CREATED);
    let body: Value = response.json();
    let instance_id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["state"], "WAITING_FOR_TASK");

    // List tasks — should have 2
    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    let tasks_arr = tasks.as_array().unwrap();
    assert_eq!(tasks_arr.len(), 2, "Expected 2 tasks after parallel fork");

    let element_ids: Vec<&str> = tasks_arr
        .iter()
        .map(|t| t["element_id"].as_str().unwrap())
        .collect();
    assert!(element_ids.contains(&"task_a"));
    assert!(element_ids.contains(&"task_b"));
}

#[tokio::test]
async fn test_parallel_partial_join_stays_active() {
    let (server, _container) = setup().await;

    // Deploy + start
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(PARALLEL_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "parallel-test" }))
        .await;
    let body: Value = response.json();
    let instance_id = body["id"].as_str().unwrap().to_string();

    // List tasks and find task_a
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

    // Claim and complete task_a
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

    // Instance should still be active (waiting for task_b)
    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_ne!(
        inst["state"], "COMPLETED",
        "Instance should not complete with only one branch done"
    );
    assert_eq!(inst["state"], "WAITING_FOR_TASK");

    // task_b should still be CREATED
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
    assert_eq!(active[0]["element_id"], "task_b");
}

#[tokio::test]
async fn test_parallel_both_complete_finishes_instance() {
    let (server, _container) = setup().await;

    // Deploy + start
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(PARALLEL_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "parallel-test" }))
        .await;
    let body: Value = response.json();
    let instance_id = body["id"].as_str().unwrap().to_string();

    // List tasks
    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();

    // Claim and complete both tasks
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

    // Instance should be COMPLETED
    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "COMPLETED");
}

#[tokio::test]
async fn test_parallel_no_duplicate_tasks_after_first_complete() {
    let (server, _container) = setup().await;

    // Deploy + start
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(PARALLEL_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "parallel-test" }))
        .await;
    let body: Value = response.json();
    let instance_id = body["id"].as_str().unwrap().to_string();

    // Verify initial state: exactly 2 tasks
    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    assert_eq!(tasks.as_array().unwrap().len(), 2);

    // Complete task_a
    let task_a = tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["element_id"] == "task_a")
        .unwrap();
    let task_a_id = task_a["id"].as_str().unwrap().to_string();

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

    // List ALL tasks (the endpoint returns all states by default).
    // There should still be exactly 2 total tasks: task_a (COMPLETED) + task_b (CREATED).
    // If the delta tracking is broken, a duplicate task_b would appear (3 tasks).
    let all_tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    let all_arr = all_tasks.as_array().unwrap();
    assert_eq!(
        all_arr.len(),
        2,
        "Completing task_a must not create a duplicate task_b"
    );

    // Verify one is completed, one is still active
    let completed: Vec<&Value> = all_arr
        .iter()
        .filter(|t| t["state"] == "COMPLETED")
        .collect();
    let created: Vec<&Value> = all_arr.iter().filter(|t| t["state"] == "CREATED").collect();
    assert_eq!(completed.len(), 1);
    assert_eq!(created.len(), 1);
    assert_eq!(created[0]["element_id"], "task_b");
}
