use axum::http::StatusCode;
use axum_test::TestServer;
use serde_json::{json, Value};
use sqlx::PgPool;
use testcontainers_modules::{postgres::Postgres, testcontainers::runners::AsyncRunner};
use uuid::Uuid;

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

async fn setup_with_scheduler() -> (TestServer, impl Drop) {
    let container = Postgres::default().start().await.unwrap();
    let host_port = container.get_host_port_ipv4(5432).await.unwrap();
    let db_url = format!("postgres://postgres:postgres@127.0.0.1:{host_port}/postgres");

    let pool = PgPool::connect(&db_url).await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    let scheduler_pool = pool.clone();
    tokio::spawn(async move {
        orrery_server::scheduler::run(scheduler_pool, std::time::Duration::from_secs(2)).await;
    });

    let app = orrery_server::build_app(pool);
    let server = TestServer::new(app).unwrap();

    (server, container)
}

const GATEWAY_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="gw">
  <process id="gw-process" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="gw1"/>
    <exclusiveGateway id="gw1">
      <outgoing>sf2</outgoing>
      <outgoing>sf3</outgoing>
    </exclusiveGateway>
    <sequenceFlow id="sf2" sourceRef="gw1" targetRef="task_a">
      <conditionExpression>approved == true</conditionExpression>
    </sequenceFlow>
    <sequenceFlow id="sf3" sourceRef="gw1" targetRef="task_b"/>
    <serviceTask id="task_a"><outgoing>sf4</outgoing></serviceTask>
    <serviceTask id="task_b"><outgoing>sf5</outgoing></serviceTask>
    <sequenceFlow id="sf4" sourceRef="task_a" targetRef="end"/>
    <sequenceFlow id="sf5" sourceRef="task_b" targetRef="end"/>
    <endEvent id="end"></endEvent>
  </process>
</definitions>"#;

const SERVICE_TASK_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="svc">
  <process id="svc-process" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1" name="Do Work"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"></endEvent>
  </process>
</definitions>"#;

const SIMPLE_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="simple">
  <process id="simple-process" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="end"/>
    <endEvent id="end"></endEvent>
  </process>
</definitions>"#;

#[tokio::test]
async fn deploy_and_get_process_definition() {
    let (server, _container) = setup().await;

    // Deploy
    let response = server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SIMPLE_BPMN)
        .await;

    response.assert_status(StatusCode::CREATED);
    let body: Value = response.json();
    assert_eq!(body["id"], "simple-process");
    assert_eq!(body["version"], 1);

    // Get
    let response = server.get("/v1/process-definitions/simple-process").await;

    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["id"], "simple-process");
}

#[tokio::test]
async fn start_process_instance() {
    let (server, _container) = setup().await;

    // Deploy first
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SIMPLE_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start instance
    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "simple-process" }))
        .await;

    response.assert_status(StatusCode::CREATED);
    let body: Value = response.json();
    assert_eq!(body["process_definition_id"], "simple-process");
    assert!(body["state"].is_string());

    // Get instance
    let id = body["id"].as_str().unwrap();
    let response = server.get(&format!("/v1/process-instances/{id}")).await;

    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["id"], id);
}

#[tokio::test]
async fn get_missing_definition_returns_404() {
    let (server, _container) = setup().await;

    let response = server.get("/v1/process-definitions/does-not-exist").await;

    response.assert_status_not_found();
}

#[tokio::test]
async fn complete_task_on_completed_instance_returns_409() {
    let (server, _container) = setup().await;

    // Use SIMPLE_BPMN (no service task) — completes immediately
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SIMPLE_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "simple-process" }))
        .await;
    response.assert_status(StatusCode::CREATED);
    let body: Value = response.json();
    assert_eq!(body["state"], "COMPLETED");

    // No tasks should exist (process completed immediately)
    let tasks: Value = server
        .get(&format!(
            "/v1/tasks?instance_id={}",
            body["id"].as_str().unwrap()
        ))
        .await
        .json();
    assert_eq!(tasks.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn complete_service_task_advances_instance_to_completed() {
    let (server, _container) = setup().await;

    // Deploy
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SERVICE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start — should pause at "task1"
    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "svc-process" }))
        .await;
    response.assert_status(StatusCode::CREATED);
    let body: Value = response.json();
    assert_eq!(body["state"], "WAITING_FOR_TASK");
    let instance_id = body["id"].as_str().unwrap().to_string();

    // List tasks — should have one CREATED task
    let response = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await;
    response.assert_status_ok();
    let tasks: Value = response.json();
    assert_eq!(tasks.as_array().unwrap().len(), 1);
    let task_id = tasks[0]["id"].as_str().unwrap().to_string();
    assert_eq!(tasks[0]["state"], "CREATED");
    assert_eq!(tasks[0]["element_id"], "task1");

    // Claim
    let response = server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({ "claimed_by": "worker-1" }))
        .await;
    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["state"], "CLAIMED");
    assert_eq!(body["claimed_by"], "worker-1");

    // Complete
    let response = server
        .post(&format!("/v1/tasks/{task_id}/complete"))
        .json(&json!({ "variables": { "result": "ok" } }))
        .await;
    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["state"], "COMPLETED");

    // Instance should now be COMPLETED
    let response = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await;
    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["state"], "COMPLETED");
    assert_eq!(body["variables"]["result"], "ok");
}

#[tokio::test]
async fn double_claim_returns_409() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SERVICE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "svc-process" }))
        .await;
    let instance_id = response.json::<Value>()["id"].as_str().unwrap().to_string();

    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    let task_id = tasks[0]["id"].as_str().unwrap().to_string();

    server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({ "claimed_by": "worker-1" }))
        .await
        .assert_status_ok();

    server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({ "claimed_by": "worker-2" }))
        .await
        .assert_status(StatusCode::CONFLICT);
}

#[tokio::test]
async fn complete_unclaimed_task_returns_409() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SERVICE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "svc-process" }))
        .await;
    let instance_id = response.json::<Value>()["id"].as_str().unwrap().to_string();

    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    let task_id = tasks[0]["id"].as_str().unwrap().to_string();

    // Complete without claiming first
    server
        .post(&format!("/v1/tasks/{task_id}/complete"))
        .json(&json!({ "variables": {} }))
        .await
        .assert_status(StatusCode::CONFLICT);
}

#[tokio::test]
async fn fail_task_sets_instance_to_failed() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SERVICE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "svc-process" }))
        .await;
    let instance_id = response.json::<Value>()["id"].as_str().unwrap().to_string();

    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    let task_id = tasks[0]["id"].as_str().unwrap().to_string();

    server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({ "claimed_by": "worker-1" }))
        .await
        .assert_status_ok();

    server
        .post(&format!("/v1/tasks/{task_id}/fail"))
        .json(&json!({ "reason": "external service unavailable" }))
        .await
        .assert_status_ok();

    let body: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(body["state"], "FAILED");
}

#[tokio::test]
async fn get_instance_history_returns_events() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SERVICE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "svc-process" }))
        .await;
    let instance_id = response.json::<Value>()["id"].as_str().unwrap().to_string();

    // Default level (activity) — should include start, task, but not gateways
    let history: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/history"))
        .await
        .json();

    let entries = history.as_array().unwrap();
    assert!(
        entries.len() >= 3,
        "history should include start activated+completed and task activated"
    );

    // Verify element_name and ordering fields are present
    assert!(
        entries[0].get("element_name").is_some(),
        "element_name field should be present"
    );
    assert!(
        entries[0].get("ordering").is_some(),
        "ordering field should be present"
    );

    // First entry should be start event activated
    assert_eq!(entries[0]["element_id"], "start");
    assert_eq!(entries[0]["event_type"], "ELEMENT_ACTIVATED");
    assert_eq!(entries[0]["element_type"], "StartEvent");

    // Should contain task1 activated
    let task_entries: Vec<&Value> = entries
        .iter()
        .filter(|e| e["element_id"] == "task1")
        .collect();
    assert!(!task_entries.is_empty(), "task1 should appear in history");
    assert_eq!(task_entries[0]["event_type"], "ELEMENT_ACTIVATED");
}

#[tokio::test]
async fn history_full_level_includes_gateways() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(GATEWAY_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "gw-process",
            "variables": { "amount": 150 }
        }))
        .await;
    let instance_id = response.json::<Value>()["id"].as_str().unwrap().to_string();

    // Default (activity) level — should NOT include gateways
    let activity_history: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/history"))
        .await
        .json();
    let activity_entries = activity_history.as_array().unwrap();
    let gateway_in_activity: Vec<&Value> = activity_entries
        .iter()
        .filter(|e| e["element_type"].as_str().unwrap_or("").contains("Gateway"))
        .collect();
    assert!(
        gateway_in_activity.is_empty(),
        "activity level should exclude gateways"
    );

    // Full level — should include gateways
    let full_history: Value = server
        .get(&format!(
            "/v1/process-instances/{instance_id}/history?level=full"
        ))
        .await
        .json();
    let full_entries = full_history.as_array().unwrap();
    let gateway_in_full: Vec<&Value> = full_entries
        .iter()
        .filter(|e| e["element_type"].as_str().unwrap_or("").contains("Gateway"))
        .collect();
    assert!(
        !gateway_in_full.is_empty(),
        "full level should include gateways"
    );
    assert!(
        full_entries.len() > activity_entries.len(),
        "full level should have more entries than activity"
    );
}

#[tokio::test]
async fn history_includes_script_task_events() {
    let (server, _container) = setup().await;

    let script_bpmn = r#"<?xml version="1.0" encoding="UTF-8"?>
    <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="script">
      <process id="script-process" isExecutable="true">
        <startEvent id="start"><outgoing>f1</outgoing></startEvent>
        <scriptTask id="script1" name="Compute" scriptFormat="rhai">
          <incoming>f1</incoming><outgoing>f2</outgoing>
          <script>1 + 1</script>
        </scriptTask>
        <endEvent id="end"><incoming>f2</incoming></endEvent>
        <sequenceFlow id="f1" sourceRef="start" targetRef="script1"/>
        <sequenceFlow id="f2" sourceRef="script1" targetRef="end"/>
      </process>
    </definitions>"#;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(script_bpmn)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "script-process" }))
        .await;
    let instance_id = response.json::<Value>()["id"].as_str().unwrap().to_string();

    let history: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/history"))
        .await
        .json();

    let entries = history.as_array().unwrap();
    // Script task should appear with both ACTIVATED and COMPLETED
    let script_entries: Vec<&Value> = entries
        .iter()
        .filter(|e| e["element_id"] == "script1")
        .collect();
    assert!(
        script_entries.len() >= 2,
        "script task should have ACTIVATED and COMPLETED events"
    );
    let events: Vec<&str> = script_entries
        .iter()
        .map(|e| e["event_type"].as_str().unwrap())
        .collect();
    assert!(
        events.contains(&"ELEMENT_ACTIVATED"),
        "should have ACTIVATED"
    );
    assert!(
        events.contains(&"ELEMENT_COMPLETED"),
        "should have COMPLETED"
    );
    // Check element_name is populated
    assert_eq!(script_entries[0]["element_name"], "Compute");
}

#[tokio::test]
async fn history_task_completion_records_completed_and_next_activated() {
    let (server, _container) = setup().await;

    // Deploy a two-task process: start -> task1 -> task2 -> end
    let bpmn = r#"<?xml version="1.0" encoding="UTF-8"?>
    <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
      <process id="two-task" isExecutable="true">
        <startEvent id="start"><outgoing>f1</outgoing></startEvent>
        <serviceTask id="task1" name="First"><incoming>f1</incoming><outgoing>f2</outgoing></serviceTask>
        <serviceTask id="task2" name="Second"><incoming>f2</incoming><outgoing>f3</outgoing></serviceTask>
        <endEvent id="end"><incoming>f3</incoming></endEvent>
        <sequenceFlow id="f1" sourceRef="start" targetRef="task1"/>
        <sequenceFlow id="f2" sourceRef="task1" targetRef="task2"/>
        <sequenceFlow id="f3" sourceRef="task2" targetRef="end"/>
      </process>
    </definitions>"#;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(bpmn)
        .await
        .assert_status(StatusCode::CREATED);
    let inst = server
        .post("/v1/process-instances")
        .json(&json!({"process_definition_id":"two-task"}))
        .await;
    let instance_id = inst.json::<Value>()["id"].as_str().unwrap().to_string();

    // Get task1 and complete it
    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    let task_id = tasks[0]["id"].as_str().unwrap();
    server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({"claimed_by":"w1"}))
        .await
        .assert_status(StatusCode::OK);
    server
        .post(&format!("/v1/tasks/{task_id}/complete"))
        .json(&json!({"variables":{}}))
        .await
        .assert_status(StatusCode::OK);

    // History should show task1 COMPLETED and task2 ACTIVATED
    let history: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/history"))
        .await
        .json();
    let entries = history.as_array().unwrap();

    let task1_completed = entries
        .iter()
        .find(|e| e["element_id"] == "task1" && e["event_type"] == "ELEMENT_COMPLETED");
    assert!(
        task1_completed.is_some(),
        "task1 should have COMPLETED event, got: {entries:?}"
    );

    let task2_activated = entries
        .iter()
        .find(|e| e["element_id"] == "task2" && e["event_type"] == "ELEMENT_ACTIVATED");
    assert!(
        task2_activated.is_some(),
        "task2 should have ACTIVATED event after task1 completion, got: {entries:?}"
    );
}

#[tokio::test]
async fn history_task_failure_records_error_thrown() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SERVICE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);
    let inst = server
        .post("/v1/process-instances")
        .json(&json!({"process_definition_id":"svc-process"}))
        .await;
    let instance_id = inst.json::<Value>()["id"].as_str().unwrap().to_string();

    // Get task and fail it
    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    let task_id = tasks[0]["id"].as_str().unwrap();
    server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({"claimed_by":"w1"}))
        .await
        .assert_status(StatusCode::OK);
    server
        .post(&format!("/v1/tasks/{task_id}/fail"))
        .json(&json!({"reason":"something broke"}))
        .await
        .assert_status(StatusCode::OK);

    // History should contain ERROR_THROWN for task1
    let history: Value = server
        .get(&format!(
            "/v1/process-instances/{instance_id}/history?level=full"
        ))
        .await
        .json();
    let entries = history.as_array().unwrap();

    let error_entry = entries
        .iter()
        .find(|e| e["element_id"] == "task1" && e["event_type"] == "ERROR_THROWN");
    assert!(
        error_entry.is_some(),
        "task1 should have ERROR_THROWN event, got: {entries:?}"
    );
}

#[tokio::test]
async fn history_ordering_is_sequential_within_engine_call() {
    let (server, _container) = setup().await;

    // Script process runs start -> script -> end in a single engine call
    let script_bpmn = r#"<?xml version="1.0" encoding="UTF-8"?>
    <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
      <process id="ord-process" isExecutable="true">
        <startEvent id="start"><outgoing>f1</outgoing></startEvent>
        <scriptTask id="s1" scriptFormat="rhai"><incoming>f1</incoming><outgoing>f2</outgoing><script>1</script></scriptTask>
        <endEvent id="end"><incoming>f2</incoming></endEvent>
        <sequenceFlow id="f1" sourceRef="start" targetRef="s1"/>
        <sequenceFlow id="f2" sourceRef="s1" targetRef="end"/>
      </process>
    </definitions>"#;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(script_bpmn)
        .await
        .assert_status(StatusCode::CREATED);
    let inst = server
        .post("/v1/process-instances")
        .json(&json!({"process_definition_id":"ord-process"}))
        .await;
    let instance_id = inst.json::<Value>()["id"].as_str().unwrap().to_string();

    let history: Value = server
        .get(&format!(
            "/v1/process-instances/{instance_id}/history?level=full"
        ))
        .await
        .json();
    let entries = history.as_array().unwrap();

    // Ordering values should be strictly increasing (0, 1, 2, ...)
    let orderings: Vec<i64> = entries
        .iter()
        .map(|e| e["ordering"].as_i64().unwrap())
        .collect();
    for window in orderings.windows(2) {
        assert!(
            window[0] <= window[1],
            "ordering should be non-decreasing, got: {orderings:?}"
        );
    }
    assert!(
        orderings.len() >= 4,
        "should have at least 4 events (start A+C, script A+C), got: {orderings:?}"
    );
}

#[tokio::test]
async fn cancel_running_instance() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SERVICE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "svc-process" }))
        .await;
    let instance_id = response.json::<Value>()["id"].as_str().unwrap().to_string();

    // Cancel it
    server
        .post(&format!("/v1/process-instances/{instance_id}/cancel"))
        .await
        .assert_status_ok();

    // Verify state is CANCELLED
    let body: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(body["state"], "CANCELLED");
    assert_eq!(body["active_element_ids"], json!([]));
}

#[tokio::test]
async fn cancel_instance_also_cancels_open_tasks() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SERVICE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "svc-process" }))
        .await;
    let instance_id = response.json::<Value>()["id"].as_str().unwrap().to_string();

    // Verify a task was created
    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    assert_eq!(tasks.as_array().unwrap().len(), 1);
    assert_eq!(tasks[0]["state"], "CREATED");
    let task_id = tasks[0]["id"].as_str().unwrap().to_string();

    // Cancel the instance
    server
        .post(&format!("/v1/process-instances/{instance_id}/cancel"))
        .await
        .assert_status_ok();

    // The task should now be CANCELLED
    let task: Value = server.get(&format!("/v1/tasks/{task_id}")).await.json();
    assert_eq!(task["state"], "CANCELLED");
}

#[tokio::test]
async fn cancel_completed_instance_returns_409() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SIMPLE_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "simple-process" }))
        .await;
    let instance_id = response.json::<Value>()["id"].as_str().unwrap().to_string();
    // simple-process completes immediately

    server
        .post(&format!("/v1/process-instances/{instance_id}/cancel"))
        .await
        .assert_status(StatusCode::CONFLICT);
}

#[tokio::test]
async fn failed_task_retries_when_max_retries_set() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SERVICE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "svc-process", "variables": {}, "max_retries": 2 }))
        .await;
    response.assert_status(StatusCode::CREATED);
    let instance_id = response.json::<Value>()["id"].as_str().unwrap().to_string();

    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    assert_eq!(tasks[0]["max_retries"], 2);
    let task_id = tasks[0]["id"].as_str().unwrap().to_string();

    server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({ "claimed_by": "worker-1" }))
        .await
        .assert_status_ok();

    // First fail — retry budget allows it, instance stays WAITING_FOR_TASK
    let fail_resp = server
        .post(&format!("/v1/tasks/{task_id}/fail"))
        .json(&json!({ "reason": "transient error" }))
        .await;
    fail_resp.assert_status_ok();
    let body: Value = fail_resp.json();
    assert_eq!(body["state"], "FAILED");
    assert_eq!(body["retry_count"], 1);

    // Instance should still be WAITING_FOR_TASK (new task created)
    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_TASK");

    // New CREATED task should exist
    let tasks2: Value = server
        .get(&format!(
            "/v1/tasks?instance_id={instance_id}&state=CREATED"
        ))
        .await
        .json();
    assert_eq!(tasks2.as_array().unwrap().len(), 1);
    assert_eq!(tasks2[0]["retry_count"], 0);
    assert_eq!(tasks2[0]["max_retries"], 1); // remaining retries
}

#[tokio::test]
async fn failed_task_with_no_retries_fails_instance() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SERVICE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Default max_retries=0
    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "svc-process" }))
        .await;
    let instance_id = response.json::<Value>()["id"].as_str().unwrap().to_string();

    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    let task_id = tasks[0]["id"].as_str().unwrap().to_string();

    server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({ "claimed_by": "worker-1" }))
        .await
        .assert_status_ok();

    server
        .post(&format!("/v1/tasks/{task_id}/fail"))
        .json(&json!({ "reason": "fatal error" }))
        .await
        .assert_status_ok();

    // Instance should be FAILED (no retries)
    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "FAILED");
}

#[tokio::test]
async fn retry_full_lifecycle_same_element_then_failed() {
    let (server, _container) = setup().await;

    // Deploy the simple service task BPMN (no boundary event)
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SERVICE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start instance with max_retries=1
    let resp = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "svc-process", "variables": {}, "max_retries": 1 }))
        .await;
    resp.assert_status(StatusCode::CREATED);
    let instance_id = resp.json::<Value>()["id"].as_str().unwrap().to_string();

    // --- Attempt 1: claim and fail (retry budget: 1 remaining -> 0) ---
    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    let task_id_1 = tasks[0]["id"].as_str().unwrap().to_string();
    let element_id = tasks[0]["element_id"].as_str().unwrap().to_string();

    server
        .post(&format!("/v1/tasks/{task_id_1}/claim"))
        .json(&json!({ "claimed_by": "worker-1" }))
        .await
        .assert_status_ok();

    server
        .post(&format!("/v1/tasks/{task_id_1}/fail"))
        .json(&json!({ "reason": "transient" }))
        .await
        .assert_status_ok();

    // Instance still waiting — retry path created a new task for same element
    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_TASK");

    let tasks2: Value = server
        .get(&format!(
            "/v1/tasks?instance_id={instance_id}&state=CREATED"
        ))
        .await
        .json();
    assert_eq!(tasks2.as_array().unwrap().len(), 1, "expected one new task");
    // New task must be for the same element
    assert_eq!(tasks2[0]["element_id"].as_str().unwrap(), element_id);
    assert_eq!(tasks2[0]["max_retries"], 0); // budget exhausted for next attempt

    // --- Attempt 2: claim and fail (retries exhausted -> engine fail_task) ---
    let task_id_2 = tasks2[0]["id"].as_str().unwrap().to_string();

    server
        .post(&format!("/v1/tasks/{task_id_2}/claim"))
        .json(&json!({ "claimed_by": "worker-1" }))
        .await
        .assert_status_ok();

    server
        .post(&format!("/v1/tasks/{task_id_2}/fail"))
        .json(&json!({ "reason": "permanent" }))
        .await
        .assert_status_ok();

    // Instance must be FAILED (no boundary event in SERVICE_TASK_BPMN)
    let inst2: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(
        inst2["state"], "FAILED",
        "instance should be FAILED after retries exhausted"
    );

    // No new tasks should have been created
    let tasks3: Value = server
        .get(&format!(
            "/v1/tasks?instance_id={instance_id}&state=CREATED"
        ))
        .await
        .json();
    assert_eq!(
        tasks3.as_array().unwrap().len(),
        0,
        "no new tasks should exist after retry exhaustion"
    );
}

#[tokio::test]
async fn exclusive_gateway_routes_by_variable() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(GATEWAY_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start with approved=true → should land on task_a
    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "gw-process", "variables": { "approved": true } }))
        .await;
    response.assert_status(StatusCode::CREATED);
    let body: Value = response.json();
    assert_eq!(body["state"], "WAITING_FOR_TASK");
    let active = body["active_element_ids"].as_array().unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0], "task_a");
}

#[tokio::test]
async fn exclusive_gateway_uses_fallback_flow() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(GATEWAY_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // No approved variable → fallback to task_b (unconditional sf3)
    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "gw-process" }))
        .await;
    response.assert_status(StatusCode::CREATED);
    let body: Value = response.json();
    assert_eq!(body["state"], "WAITING_FOR_TASK");
    let active = body["active_element_ids"].as_array().unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0], "task_b");
}

const TASK_THEN_TIMER_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="ttt">
  <process id="task-then-timer-process" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="timer1"/>
    <intermediateCatchEvent id="timer1">
      <outgoing>sf3</outgoing>
      <timerEventDefinition>
        <timeDuration>PT1H</timeDuration>
      </timerEventDefinition>
    </intermediateCatchEvent>
    <sequenceFlow id="sf3" sourceRef="timer1" targetRef="end"/>
    <endEvent id="end"></endEvent>
  </process>
</definitions>"#;

const TIMER_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="timerd">
  <process id="timer-process" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="timer1"/>
    <intermediateCatchEvent id="timer1">
      <outgoing>sf2</outgoing>
      <timerEventDefinition>
        <timeDuration>PT1S</timeDuration>
      </timerEventDefinition>
    </intermediateCatchEvent>
    <sequenceFlow id="sf2" sourceRef="timer1" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>sf3</outgoing></serviceTask>
    <sequenceFlow id="sf3" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"></endEvent>
  </process>
</definitions>"#;

#[tokio::test]
async fn timer_start_puts_instance_in_waiting_for_timer_state() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(TIMER_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "timer-process" }))
        .await;
    response.assert_status(StatusCode::CREATED);
    let body: Value = response.json();
    assert_eq!(body["state"], "WAITING_FOR_TIMER");
    let active = body["active_element_ids"].as_array().unwrap();
    assert_eq!(active[0], "timer1");
}

/// Verifies that the scheduler fires a due timer and advances the instance.
/// Uses a 2-second scheduler tick; total wait is ~4 seconds.
#[tokio::test]
async fn timer_fires_and_advances_instance() {
    let (server, _container) = setup_with_scheduler().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(TIMER_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "timer-process" }))
        .await;
    response.assert_status(StatusCode::CREATED);
    let instance_id = response.json::<Value>()["id"].as_str().unwrap().to_string();

    assert_eq!(
        server
            .get(&format!("/v1/process-instances/{instance_id}"))
            .await
            .json::<Value>()["state"],
        "WAITING_FOR_TIMER"
    );

    // Wait for timer (1s) + scheduler tick (2s) + buffer
    tokio::time::sleep(tokio::time::Duration::from_secs(4)).await;

    let body: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(body["state"], "WAITING_FOR_TASK");
    let active = body["active_element_ids"].as_array().unwrap();
    assert_eq!(active[0], "task1");
}

/// Verifies that fast-forwarding a timer records ELEMENT_COMPLETED for the timer element
/// and ELEMENT_ACTIVATED for the next element in the execution history.
#[tokio::test]
async fn timer_fast_forward_records_history() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(TIMER_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let inst_body: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "timer-process" }))
        .await
        .json();
    assert_eq!(inst_body["state"], "WAITING_FOR_TIMER");
    let instance_id = inst_body["id"].as_str().unwrap().to_string();

    let timers: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/timers"))
        .await
        .json();
    let timer_id = timers[0]["id"].as_str().unwrap().to_string();

    // Fast-forward the timer
    server
        .post(&format!(
            "/v1/process-instances/{instance_id}/timers/{timer_id}/fast-forward"
        ))
        .await
        .assert_status(StatusCode::NO_CONTENT);

    let history: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/history"))
        .await
        .json();
    let entries = history.as_array().unwrap();

    // Should contain: timer1 ELEMENT_ACTIVATED (on start), timer1 ELEMENT_COMPLETED (on fire),
    // task1 ELEMENT_ACTIVATED (after timer fires)
    let completed = entries
        .iter()
        .find(|e| e["element_id"] == "timer1" && e["event_type"] == "ELEMENT_COMPLETED");
    assert!(
        completed.is_some(),
        "expected ELEMENT_COMPLETED for timer1, got: {entries:?}"
    );

    let activated = entries
        .iter()
        .find(|e| e["element_id"] == "task1" && e["event_type"] == "ELEMENT_ACTIVATED");
    assert!(
        activated.is_some(),
        "expected ELEMENT_ACTIVATED for task1, got: {entries:?}"
    );
}

/// Verifies that after a task completes and transitions to a timer intermediate event,
/// both active_element_ids and scheduled_timers are consistent (atomic update).
#[tokio::test]
async fn task_completion_to_timer_has_consistent_state() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(TASK_THEN_TIMER_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let response = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "task-then-timer-process" }))
        .await;
    response.assert_status(StatusCode::CREATED);
    let body: Value = response.json();
    let instance_id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["state"], "WAITING_FOR_TASK");

    // Get and claim the task
    let tasks_resp = server.get("/v1/tasks").await.json::<Value>();
    let tasks = tasks_resp.as_array().unwrap();
    let task_id = tasks[0]["id"].as_str().unwrap().to_string();

    server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({ "claimed_by": "test-worker" }))
        .await
        .assert_status(StatusCode::OK);

    // Complete the task — triggers transition to timer
    server
        .post(&format!("/v1/tasks/{task_id}/complete"))
        .json(&json!({ "variables": {} }))
        .await
        .assert_status(StatusCode::OK);

    // 1. active_element_ids should contain "timer1"
    let inst_body: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst_body["state"], "WAITING_FOR_TIMER");
    let active = inst_body["active_element_ids"].as_array().unwrap();
    assert_eq!(active[0], "timer1");

    // 2. scheduled_timers should have a row for timer1
    let timers_resp: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/timers"))
        .await
        .json();
    let timers = timers_resp.as_array().unwrap();
    assert_eq!(timers.len(), 1);
    assert_eq!(timers[0]["element_id"], "timer1");
}

#[tokio::test]
async fn crash_recovery_creates_task_for_orphaned_instance() {
    let container = Postgres::default().start().await.unwrap();
    let host_port = container.get_host_port_ipv4(5432).await.unwrap();
    let db_url = format!("postgres://postgres:postgres@127.0.0.1:{host_port}/postgres");
    let pool = PgPool::connect(&db_url).await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    // Manually insert a WAITING_FOR_TASK instance with no task (simulates a crashed server)
    sqlx::query(
        "INSERT INTO process_definitions (id, version, bpmn_xml) VALUES ('crash-def', 1, $1)",
    )
    .bind(SERVICE_TASK_BPMN)
    .execute(&pool)
    .await
    .unwrap();

    let instance_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO process_instances (id, process_definition_id, process_definition_version, state, variables, active_element_ids) \
         VALUES ($1, 'crash-def', 1, 'WAITING_FOR_TASK', '{}', '[\"task1\"]')"
    )
    .bind(&instance_id)
    .execute(&pool)
    .await
    .unwrap();

    // No task exists yet
    let rows = sqlx::query("SELECT id FROM tasks WHERE process_instance_id = $1")
        .bind(&instance_id)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(rows.len(), 0);

    // Run recovery
    let recovered = orrery_server::recovery::recover_orphaned_tasks(&pool)
        .await
        .unwrap();
    assert_eq!(recovered, 1);

    // Task should now exist
    let rows = sqlx::query("SELECT id FROM tasks WHERE process_instance_id = $1")
        .bind(&instance_id)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
}

// ─── Message correlation tests ────────────────────────────────────────────────

const MESSAGE_CATCH_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:zeebe="http://camunda.org/schema/zeebe/1.0" id="msgd">
  <message id="Msg_OrderApproved" name="OrderApproved">
    <extensionElements>
      <zeebe:subscription correlationKey="= order_id"/>
    </extensionElements>
  </message>
  <process id="msg-process" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="msg_wait"/>
    <intermediateCatchEvent id="msg_wait">
      <messageEventDefinition messageRef="Msg_OrderApproved"/>
      <outgoing>sf2</outgoing>
    </intermediateCatchEvent>
    <sequenceFlow id="sf2" sourceRef="msg_wait" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>sf3</outgoing></serviceTask>
    <sequenceFlow id="sf3" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"></endEvent>
  </process>
</definitions>"#;

#[tokio::test]
async fn send_message_wakes_waiting_instance() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(MESSAGE_CATCH_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start — should pause at msg_wait
    let body: Value = server
        .post("/v1/process-instances")
        .json(
            &json!({ "process_definition_id": "msg-process", "variables": { "order_id": "123" } }),
        )
        .await
        .json();
    assert_eq!(body["state"], "WAITING_FOR_MESSAGE");
    let active = body["active_element_ids"].as_array().unwrap();
    assert_eq!(active[0], "msg_wait");
    let instance_id = body["id"].as_str().unwrap().to_string();

    // Send message — no correlation filter needed (orderId matches via subscription)
    let raw = server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "OrderApproved",
            "correlation_key": "123",
            "variables": { "approval_code": "A99" }
        }))
        .await;
    let status = raw.status_code();
    let body_text = raw.text();
    eprintln!("DEBUG status={status} body={body_text:?}");
    let resp: Value = serde_json::from_str(&body_text).expect("expected JSON body");
    assert_eq!(resp["process_instance_id"], instance_id);
    assert_eq!(resp["instance_state"], "WAITING_FOR_TASK");

    // Instance is now at task1 with merged variables
    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_TASK");
    let active = inst["active_element_ids"].as_array().unwrap();
    assert_eq!(active[0], "task1");
    assert_eq!(inst["variables"]["approval_code"], "A99");
}

#[tokio::test]
async fn send_message_with_correlation_matches_correct_instance() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(MESSAGE_CATCH_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start two instances with different order IDs
    let inst_a: Value = server
        .post("/v1/process-instances")
        .json(
            &json!({ "process_definition_id": "msg-process", "variables": { "order_id": "AAA" } }),
        )
        .await
        .json();
    assert_eq!(inst_a["state"], "WAITING_FOR_MESSAGE");
    let id_a = inst_a["id"].as_str().unwrap().to_string();

    let inst_b: Value = server
        .post("/v1/process-instances")
        .json(
            &json!({ "process_definition_id": "msg-process", "variables": { "order_id": "BBB" } }),
        )
        .await
        .json();
    assert_eq!(inst_b["state"], "WAITING_FOR_MESSAGE");
    let id_b = inst_b["id"].as_str().unwrap().to_string();

    // Send message correlated to order_id BBB — should only wake instance B
    let resp: Value = server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "OrderApproved",
            "correlation_key": "BBB"
        }))
        .await
        .json();
    assert_eq!(resp["process_instance_id"], id_b);

    // Instance A must still be WAITING_FOR_MESSAGE
    let a: Value = server
        .get(&format!("/v1/process-instances/{id_a}"))
        .await
        .json();
    assert_eq!(a["state"], "WAITING_FOR_MESSAGE");

    // Instance B is now WAITING_FOR_TASK
    let b: Value = server
        .get(&format!("/v1/process-instances/{id_b}"))
        .await
        .json();
    assert_eq!(b["state"], "WAITING_FOR_TASK");
}

#[tokio::test]
async fn send_message_returns_404_when_no_match() {
    let (server, _container) = setup().await;

    server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "NonExistentMessage"
        }))
        .await
        .assert_status_not_found();
}

// ─── Signal broadcast tests ───────────────────────────────────────────────────

const SIGNAL_CATCH_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="sigd">
  <process id="sig-process" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="sig_wait"/>
    <intermediateCatchEvent id="sig_wait">
      <signalEventDefinition signalRef="PaymentReceived"/>
      <outgoing>sf2</outgoing>
    </intermediateCatchEvent>
    <sequenceFlow id="sf2" sourceRef="sig_wait" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>sf3</outgoing></serviceTask>
    <sequenceFlow id="sf3" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"></endEvent>
  </process>
</definitions>"#;

#[tokio::test]
async fn signal_broadcast_wakes_single_waiting_instance() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SIGNAL_CATCH_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let body: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "sig-process" }))
        .await
        .json();
    assert_eq!(body["state"], "WAITING_FOR_SIGNAL");
    let instance_id = body["id"].as_str().unwrap().to_string();

    let resp: Value = server
        .post("/v1/signals/PaymentReceived")
        .json(&json!({ "variables": { "payment_ref": "PAY-001" } }))
        .await
        .json();
    assert_eq!(resp["woken_count"], 1);
    assert_eq!(resp["process_instance_ids"][0], instance_id);

    // Instance advanced to WAITING_FOR_TASK with injected variables
    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_TASK");
    assert_eq!(inst["variables"]["payment_ref"], "PAY-001");
}

#[tokio::test]
async fn signal_broadcast_wakes_all_waiting_instances() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SIGNAL_CATCH_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start two instances both waiting for the same signal
    let id_1 = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "sig-process" }))
        .await
        .json::<Value>()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let id_2 = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "sig-process" }))
        .await
        .json::<Value>()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Broadcast — both should be woken
    let resp: Value = server
        .post("/v1/signals/PaymentReceived")
        .json(&json!({}))
        .await
        .json();
    assert_eq!(resp["woken_count"], 2);

    let woken_ids: Vec<&str> = resp["process_instance_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(woken_ids.contains(&id_1.as_str()));
    assert!(woken_ids.contains(&id_2.as_str()));

    // Both instances advanced
    for id in [&id_1, &id_2] {
        let inst: Value = server
            .get(&format!("/v1/process-instances/{id}"))
            .await
            .json();
        assert_eq!(inst["state"], "WAITING_FOR_TASK");
    }
}

#[tokio::test]
async fn signal_broadcast_returns_zero_when_no_waiters() {
    let (server, _container) = setup().await;

    let resp: Value = server
        .post("/v1/signals/UnknownSignal")
        .json(&json!({}))
        .await
        .json();
    assert_eq!(resp["woken_count"], 0);
    assert_eq!(resp["process_instance_ids"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_metrics_overview_returns_counts() {
    let (server, _container) = setup().await;

    // Empty database — all counts should be zero
    let body: Value = server.get("/v1/metrics/overview").await.json();
    assert_eq!(body["running_instances"], 0);
    assert_eq!(body["waiting_instances"], 0);
    assert_eq!(body["completed_instances"], 0);
    assert_eq!(body["failed_instances"], 0);
    assert_eq!(body["pending_tasks"], 0);
    assert_eq!(body["claimed_tasks"], 0);

    // Deploy a service-task process and start an instance.
    // The instance pauses at the task → WAITING_FOR_TASK, creates 1 CREATED task.
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SERVICE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "svc-process" }))
        .await
        .assert_status(StatusCode::CREATED);

    let body: Value = server.get("/v1/metrics/overview").await.json();
    assert_eq!(body["waiting_instances"], 1);
    assert_eq!(body["pending_tasks"], 1);
    assert_eq!(body["claimed_tasks"], 0);
    assert_eq!(body["completed_instances"], 0);

    // Also deploy and start a simple process — completes immediately → COMPLETED
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SIMPLE_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "simple-process" }))
        .await
        .assert_status(StatusCode::CREATED);

    let body: Value = server.get("/v1/metrics/overview").await.json();
    assert_eq!(body["waiting_instances"], 1);
    assert_eq!(body["completed_instances"], 1);
    assert_eq!(body["pending_tasks"], 1);
}

#[tokio::test]
async fn gateway_routes_on_greater_than_condition() {
    let (server, _container) = setup().await;

    // BPMN with two routes: amount > 1000 → big_task, otherwise → small_task
    const GT_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="gt">
  <process id="gt-process" isExecutable="true">
    <startEvent id="start"><outgoing>sf0</outgoing></startEvent>
    <sequenceFlow id="sf0" sourceRef="start" targetRef="gw1"/>
    <exclusiveGateway id="gw1">
      <outgoing>sf1</outgoing>
      <outgoing>sf2</outgoing>
    </exclusiveGateway>
    <sequenceFlow id="sf1" sourceRef="gw1" targetRef="big_task">
      <conditionExpression>${amount} &gt; 1000</conditionExpression>
    </sequenceFlow>
    <sequenceFlow id="sf2" sourceRef="gw1" targetRef="small_task"/>
    <serviceTask id="big_task"><outgoing>sf3</outgoing></serviceTask>
    <serviceTask id="small_task"><outgoing>sf4</outgoing></serviceTask>
    <sequenceFlow id="sf3" sourceRef="big_task" targetRef="end"/>
    <sequenceFlow id="sf4" sourceRef="small_task" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(GT_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // amount=5000 → should route to big_task (5000 > 1000 is true)
    let resp = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "gt-process", "variables": { "amount": 5000 } }))
        .await;
    resp.assert_status(StatusCode::CREATED);
    let instance_id = resp.json::<Value>()["id"].as_str().unwrap().to_string();

    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    assert_eq!(
        tasks[0]["element_id"].as_str().unwrap(),
        "big_task",
        "amount=5000 should route to big_task via > 1000 condition"
    );
}

const DI_BPMN: &str = include_str!("../../orrery/tests/fixtures/simple_with_di.bpmn");

#[tokio::test]
async fn test_diagram_layout_parser_extracts_bounds() {
    use orrery::diagram::parse_diagram_layout;

    let layout = parse_diagram_layout(DI_BPMN);

    let start_bounds = layout
        .shapes
        .get("StartEvent_1")
        .expect("StartEvent_1 bounds missing");
    assert_eq!(start_bounds.x, 152.0);
    assert_eq!(start_bounds.y, 82.0);
    assert_eq!(start_bounds.width, 36.0);
    assert_eq!(start_bounds.height, 36.0);

    let task_bounds = layout
        .shapes
        .get("ServiceTask_1")
        .expect("ServiceTask_1 bounds missing");
    assert_eq!(task_bounds.x, 240.0);
    assert_eq!(task_bounds.width, 100.0);

    let flow1_waypoints = layout
        .edges
        .get("Flow_1")
        .expect("Flow_1 waypoints missing");
    assert_eq!(flow1_waypoints.len(), 2);
    assert_eq!(flow1_waypoints[0].x, 188.0);
    assert_eq!(flow1_waypoints[1].x, 240.0);
}

#[tokio::test]
async fn test_diagram_renders_with_di_coordinates() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(DI_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let start = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "Process_1", "variables": {} }))
        .await;
    start.assert_status(StatusCode::CREATED);
    let instance_id = start.json::<Value>()["id"].as_str().unwrap().to_string();

    let resp = server
        .get(&format!("/v1/process-instances/{instance_id}/diagram"))
        .await;
    resp.assert_status_ok();

    let svg = resp.text();
    assert!(svg.contains("viewBox"), "SVG must have viewBox");
    assert!(svg.contains("240"), "SVG should use DI x-coordinate 240");
    assert!(
        !svg.contains("cx=\"80\""),
        "SVG should not use linear layout cx=80"
    );
}

#[tokio::test]
async fn test_diagram_badge_appears_for_active_element() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(DI_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let start = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "Process_1", "variables": {} }))
        .await;
    start.assert_status(StatusCode::CREATED);
    let instance_id = start.json::<Value>()["id"].as_str().unwrap().to_string();

    let resp = server
        .get(&format!("/v1/process-instances/{instance_id}/diagram"))
        .await;
    resp.assert_status_ok();

    let svg = resp.text();
    assert!(
        svg.contains("#3b82f6"),
        "SVG should contain blue badge for active element"
    );
}

#[tokio::test]
async fn test_diagram_fallback_without_di() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SERVICE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let start = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "svc-process", "variables": {} }))
        .await;
    start.assert_status(StatusCode::CREATED);
    let instance_id = start.json::<Value>()["id"].as_str().unwrap().to_string();

    let resp = server
        .get(&format!("/v1/process-instances/{instance_id}/diagram"))
        .await;
    resp.assert_status_ok();

    let svg = resp.text();
    assert!(svg.contains("<svg"), "Response should be SVG markup");
    assert!(svg.contains("viewBox"), "SVG must have viewBox");
}

#[tokio::test]
async fn test_list_definitions_returns_instance_counts() {
    let (server, _c) = setup().await;

    let deploy_resp = server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SERVICE_TASK_BPMN)
        .await;
    deploy_resp.assert_status(StatusCode::CREATED);

    // Start 2 instances (they will be WAITING_FOR_TASK)
    for _ in 0..2 {
        server
            .post("/v1/process-instances")
            .json(&json!({ "process_definition_id": "svc-process" }))
            .await
            .assert_status(StatusCode::CREATED);
    }

    let list = server.get("/v1/process-definitions").await;
    list.assert_status_ok();
    let body: Value = list.json();
    let def = &body["items"][0];
    // WAITING_FOR_TASK is counted as running
    assert!(def["running_count"].as_i64().unwrap() >= 2);
    assert_eq!(def["completed_count"], 0);
    assert_eq!(def["failed_count"], 0);
}

#[tokio::test]
async fn test_list_instances_filtered_by_definition_id() {
    let (server, _c) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SERVICE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SIMPLE_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "svc-process" }))
        .await
        .assert_status(StatusCode::CREATED);

    server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "simple-process" }))
        .await
        .assert_status(StatusCode::CREATED);

    let resp = server
        .get("/v1/process-instances?definition_id=svc-process")
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    let arr = body["items"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["process_definition_id"], "svc-process");
}

#[tokio::test]
async fn test_failed_instance_has_error_message() {
    let (server, _c) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SERVICE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "svc-process" }))
        .await
        .json();
    let inst_id = inst["id"].as_str().unwrap();

    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={inst_id}"))
        .await
        .json();
    let task_id = tasks[0]["id"].as_str().unwrap();

    server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({ "claimed_by": "tester" }))
        .await
        .assert_status_ok();

    server
        .post(&format!("/v1/tasks/{task_id}/fail"))
        .json(&json!({ "reason": "test failure" }))
        .await
        .assert_status_ok();

    let instance: Value = server
        .get(&format!("/v1/process-instances/{inst_id}"))
        .await
        .json();
    assert_eq!(instance["state"], "FAILED");
    let error_msg = instance["error_message"].as_str().unwrap_or("");
    assert!(
        !error_msg.is_empty(),
        "error_message should be populated on FAILED instance"
    );
    assert!(
        error_msg.contains("task1"),
        "error_message should mention the element"
    );
}

#[tokio::test]
async fn test_task_response_includes_process_definition_id() {
    let (server, _c) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SERVICE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "svc-process" }))
        .await
        .assert_status(StatusCode::CREATED);

    let tasks: Value = server.get("/v1/tasks").await.json();
    let arr = tasks.as_array().unwrap();
    assert!(!arr.is_empty());
    let def_id = arr[0]["process_definition_id"].as_str().unwrap_or("");
    assert_eq!(def_id, "svc-process");
}

#[tokio::test]
async fn retry_failed_task() {
    let (server, _container) = setup().await;

    // Deploy
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .bytes(SERVICE_TASK_BPMN.as_bytes().into())
        .await;

    // Start instance (0 max_retries so task goes straight to FAILED)
    let inst = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "svc-process", "max_retries": 0 }))
        .await;
    let inst_id = inst.json::<Value>()["id"].as_str().unwrap().to_string();

    // Get the task
    let tasks = server
        .get("/v1/tasks")
        .add_query_param("instance_id", &inst_id)
        .await
        .json::<Value>();
    let task_id = tasks[0]["id"].as_str().unwrap().to_string();

    // Claim then fail (no retries left → state = FAILED)
    server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({ "claimed_by": "worker-1" }))
        .await;
    server
        .post(&format!("/v1/tasks/{task_id}/fail"))
        .json(&json!({ "reason": "boom" }))
        .await;

    // Verify task is now FAILED
    let t = server
        .get(&format!("/v1/tasks/{task_id}"))
        .await
        .json::<Value>();
    assert_eq!(t["state"], "FAILED");
    let retry_count_before = t["retry_count"].as_i64().unwrap();

    // Retry
    let resp = server.post(&format!("/v1/tasks/{task_id}/retry")).await;
    assert_eq!(resp.status_code(), StatusCode::OK);

    let retried = resp.json::<Value>();
    assert_eq!(retried["state"], "CREATED");
    assert_eq!(
        retried["retry_count"].as_i64().unwrap(),
        retry_count_before + 1
    );
}

#[tokio::test]
async fn update_instance_variables() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .bytes(SERVICE_TASK_BPMN.as_bytes().into())
        .await;

    let inst = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "svc-process",
            "variables": { "x": 1 }
        }))
        .await
        .json::<Value>();
    let inst_id = inst["id"].as_str().unwrap().to_string();

    // Update variables — merge new key, keep existing
    let resp = server
        .put(&format!("/v1/process-instances/{inst_id}/variables"))
        .json(&json!({ "variables": { "y": 2 } }))
        .await;
    assert_eq!(resp.status_code(), StatusCode::OK);

    let updated = resp.json::<Value>();
    assert_eq!(updated["variables"]["x"], 1); // original preserved
    assert_eq!(updated["variables"]["y"], 2); // new key added
}

const EXTERNAL_TASK_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:camunda="http://camunda.org/schema/1.0/bpmn" id="ext">
  <process id="ext-process" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="ext_task"/>
    <serviceTask id="ext_task" camunda:type="external" camunda:topic="my-topic">
      <outgoing>sf2</outgoing>
    </serviceTask>
    <sequenceFlow id="sf2" sourceRef="ext_task" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;

// ─── Definition diagram endpoint tests ────────────────────────────────────────

#[tokio::test]
async fn get_instance_diagram_not_found_returns_404() {
    let (server, _container) = setup().await;

    server
        .get("/v1/process-instances/nonexistent-id/diagram")
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn get_definition_diagram_returns_svg() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(DI_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let resp = server
        .get("/v1/process-definitions/Process_1/diagram")
        .await;
    resp.assert_status_ok();

    let svg = resp.text();
    assert!(svg.contains("<svg"), "response should be SVG markup");
    assert!(
        svg.contains("viewBox"),
        "SVG should include viewBox from DI layout"
    );
}

#[tokio::test]
async fn get_definition_diagram_not_found_returns_404() {
    let (server, _container) = setup().await;

    server
        .get("/v1/process-definitions/nonexistent-def/diagram")
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn get_definition_diagram_counts_active_instances() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(DI_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start 2 instances — both pause at ServiceTask_1
    for _ in 0..2 {
        server
            .post("/v1/process-instances")
            .json(&json!({ "process_definition_id": "Process_1" }))
            .await
            .assert_status(StatusCode::CREATED);
    }

    let resp = server
        .get("/v1/process-definitions/Process_1/diagram")
        .await;
    resp.assert_status_ok();

    let svg = resp.text();
    // Badge should show count "2" and be blue (#3b82f6)
    assert!(svg.contains(">2<"), "badge should display count 2");
    assert!(
        svg.contains("#3b82f6"),
        "active instance badge should be blue"
    );
}

#[tokio::test]
async fn get_definition_diagram_failed_instances_counted_correctly() {
    // Regression test: failed instances at the same element were being
    // deduplicated before counting, showing 1 instead of N.
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(DI_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start 3 instances and fail each one at ServiceTask_1
    for _ in 0..3 {
        let resp: Value = server
            .post("/v1/process-instances")
            .json(&json!({ "process_definition_id": "Process_1" }))
            .await
            .json();
        let instance_id = resp["id"].as_str().unwrap().to_string();

        let tasks: Value = server
            .get(&format!("/v1/tasks?instance_id={instance_id}"))
            .await
            .json();
        let task_id = tasks[0]["id"].as_str().unwrap().to_string();

        server
            .post(&format!("/v1/tasks/{task_id}/claim"))
            .json(&json!({ "claimed_by": "worker-1" }))
            .await
            .assert_status_ok();

        server
            .post(&format!("/v1/tasks/{task_id}/fail"))
            .json(&json!({ "reason": "test failure" }))
            .await
            .assert_status_ok();
    }

    let resp = server
        .get("/v1/process-definitions/Process_1/diagram")
        .await;
    resp.assert_status_ok();

    let svg = resp.text();
    // All 3 failures should be counted, not deduplicated down to 1
    assert!(
        svg.contains(">3<"),
        "badge should show count 3 for 3 failed instances"
    );
    // Badge should be red for failed elements
    assert!(
        svg.contains("#ef4444"),
        "failed element badge should be red"
    );
}

#[tokio::test]
async fn get_definition_diagram_counts_waiting_for_message_instances() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(RECEIVE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start instance — pauses at receiveTask in WAITING_FOR_MESSAGE
    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "receive-task-process",
            "variables": { "orderId": "order-1" }
        }))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_MESSAGE");

    let resp = server
        .get("/v1/process-definitions/receive-task-process/diagram")
        .await;
    resp.assert_status_ok();

    let svg = resp.text();
    // The receive task element should be highlighted as active
    assert!(
        svg.contains("bpmn-active"),
        "WAITING_FOR_MESSAGE instance should appear in definition diagram"
    );
}

#[tokio::test]
async fn get_instance_diagram_marks_failed_task_element_red() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(DI_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let resp: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "Process_1" }))
        .await
        .json();
    let instance_id = resp["id"].as_str().unwrap().to_string();

    let tasks: Value = server
        .get(&format!("/v1/tasks?instance_id={instance_id}"))
        .await
        .json();
    let task_id = tasks[0]["id"].as_str().unwrap().to_string();

    server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({ "claimed_by": "worker-1" }))
        .await
        .assert_status_ok();

    server
        .post(&format!("/v1/tasks/{task_id}/fail"))
        .json(&json!({ "reason": "test failure" }))
        .await
        .assert_status_ok();

    let resp = server
        .get(&format!("/v1/process-instances/{instance_id}/diagram"))
        .await;
    resp.assert_status_ok();

    let svg = resp.text();
    assert!(
        svg.contains("bpmn-failed"),
        "failed task element should have bpmn-failed CSS class"
    );
    assert!(
        svg.contains("#ef4444"),
        "failed task element should use red color"
    );
}

#[tokio::test]
async fn external_task_failure_records_error_message() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(EXTERNAL_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let inst = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "ext-process" }))
        .await
        .json::<Value>();
    let instance_id = inst["id"].as_str().unwrap().to_string();

    // Fetch the external task via long-poll (short timeout so it doesn't hang)
    let locked: Value = server
        .post("/v1/external-tasks/fetch-and-lock")
        .json(&json!({
            "worker_id": "test-worker",
            "subscriptions": [{ "topic": "my-topic" }],
            "max_tasks": 1,
            "lock_duration_ms": 60000,
            "request_timeout_ms": 100
        }))
        .await
        .json();
    let task_id = locked[0]["id"].as_str().unwrap().to_string();

    // Report failure with a descriptive error message (simulates handler returning Err)
    server
        .post(&format!("/v1/external-tasks/{task_id}/failure"))
        .json(&json!({
            "worker_id": "test-worker",
            "error_message": "topic2 handler failed: connection refused",
            "retries": 0,
            "retry_timeout_ms": 0
        }))
        .await
        .assert_status_ok();

    // The instance should be FAILED and error_message must be recorded in the DB
    let body: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(body["state"], "FAILED");
    let error_msg = body["error_message"].as_str().unwrap_or("");
    assert!(
        error_msg.contains("topic2 handler failed: connection refused"),
        "expected error_message to contain the handler error, got: {:?}",
        error_msg
    );
}

// ─── Timer Start Event tests ───────────────────────────────────────────────────

const TIMER_START_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="tsd">
  <process id="timer-start-process" isExecutable="true">
    <startEvent id="timer_start">
      <outgoing>sf1</outgoing>
      <timerEventDefinition><timeCycle>R3/PT1H</timeCycle></timerEventDefinition>
    </startEvent>
    <sequenceFlow id="sf1" sourceRef="timer_start" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"></endEvent>
  </process>
</definitions>"#;

/// Verifies that deploying a BPMN with a TimerStartEvent registers a row in
/// timer_start_definitions with the correct kind, expression, and element_id.
#[tokio::test]
async fn deploy_timer_start_event_registers_definition() {
    let container = Postgres::default().start().await.unwrap();
    let host_port = container.get_host_port_ipv4(5432).await.unwrap();
    let db_url = format!("postgres://postgres:postgres@127.0.0.1:{host_port}/postgres");
    let pool = PgPool::connect(&db_url).await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    let app = orrery_server::build_app(pool.clone());
    let server = TestServer::new(app).unwrap();

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(TIMER_START_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let rows = sqlx::query(
        "SELECT process_def_key, element_id, timer_kind, expression, enabled \
         FROM timer_start_definitions \
         WHERE process_def_key = 'timer-start-process'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(rows.len(), 1, "expected one timer_start_definitions row");

    use sqlx::Row;
    assert_eq!(rows[0].get::<String, _>("element_id"), "timer_start");
    assert_eq!(rows[0].get::<String, _>("timer_kind"), "cycle");
    assert_eq!(rows[0].get::<String, _>("expression"), "R3/PT1H");
    assert!(rows[0].get::<bool, _>("enabled"));
}

const CYCLE_TIMER_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="cycled">
  <process id="cycle-timer-process" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="timer1"/>
    <intermediateCatchEvent id="timer1">
      <outgoing>sf2</outgoing>
      <timerEventDefinition><timeCycle>R3/PT1H</timeCycle></timerEventDefinition>
    </intermediateCatchEvent>
    <sequenceFlow id="sf2" sourceRef="timer1" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>sf3</outgoing></serviceTask>
    <sequenceFlow id="sf3" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"></endEvent>
  </process>
</definitions>"#;

/// Verifies that firing a cycle timer creates a new scheduled_timers row with
/// the repetition count decremented (R3/PT1H → R2/PT1H).
#[tokio::test]
async fn cycle_timer_reschedules_with_decremented_count_after_firing() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(CYCLE_TIMER_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start instance — pauses at cycle timer
    let body: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "cycle-timer-process" }))
        .await
        .json();
    assert_eq!(body["state"], "WAITING_FOR_TIMER");
    let instance_id = body["id"].as_str().unwrap().to_string();

    // Get the pending timer
    let timers: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/timers"))
        .await
        .json();
    let timers_arr = timers.as_array().unwrap();
    assert_eq!(timers_arr.len(), 1);
    let timer_id = timers_arr[0]["id"].as_str().unwrap().to_string();
    assert_eq!(timers_arr[0]["expression"], "R3/PT1H");

    // Fast-forward the timer — fires it immediately
    server
        .post(&format!(
            "/v1/process-instances/{instance_id}/timers/{timer_id}/fast-forward"
        ))
        .await
        .assert_status(StatusCode::NO_CONTENT);

    // After firing a cycle timer, a new scheduled_timers row should appear with
    // the decremented expression R2/PT1H
    let timers_after: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/timers"))
        .await
        .json();
    let timers_after_arr = timers_after.as_array().unwrap();
    assert_eq!(
        timers_after_arr.len(),
        2,
        "expected original (fired) + rescheduled timer, got: {timers_after_arr:?}"
    );

    let rescheduled = timers_after_arr
        .iter()
        .find(|t| t["fired"] == false)
        .expect("should have an unfired rescheduled timer");
    assert_eq!(rescheduled["expression"], "R2/PT1H");
    assert_eq!(rescheduled["element_id"], "timer1");
}

/// Regression test: fast-forwarding a timer on an instance of version N must NOT
/// accidentally load version N-1's BPMN.
///
/// v1 has a direct gateway→end path (no timer element `timer_v2`).
/// v2 replaces that path with a timer intermediate event `timer_v2` → end.
/// An instance started on v2 should be fast-forwardable to COMPLETED.
/// Before the fix, `advance_timer` queried `process_definitions WHERE id = $1`
/// without the version, so Postgres could return v1's BPMN, causing
/// `EngineError::TargetNotFound("timer_v2")` → HTTP 500.
#[tokio::test]
async fn timer_fast_forward_uses_correct_definition_version() {
    let (server, _container) = setup().await;

    // v1: start → end (no timer)
    const V1_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="d1">
  <process id="versioned-timer-process" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="end_v1"/>
    <endEvent id="end_v1"/>
  </process>
</definitions>"#;

    // v2: start → timer_v2 (different element id, doesn't exist in v1) → end
    const V2_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="d2">
  <process id="versioned-timer-process" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="timer_v2"/>
    <intermediateCatchEvent id="timer_v2">
      <outgoing>sf2</outgoing>
      <timerEventDefinition><timeDuration>PT1H</timeDuration></timerEventDefinition>
    </intermediateCatchEvent>
    <sequenceFlow id="sf2" sourceRef="timer_v2" targetRef="end_v2"/>
    <endEvent id="end_v2"/>
  </process>
</definitions>"#;

    // Deploy v1, then v2
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(V1_BPMN)
        .await
        .assert_status(StatusCode::CREATED);
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(V2_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start an instance — it should be on v2 (latest), waiting at timer_v2
    let inst_body: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "versioned-timer-process" }))
        .await
        .json();
    assert_eq!(inst_body["state"], "WAITING_FOR_TIMER");
    assert_eq!(inst_body["active_element_ids"][0], "timer_v2");
    let instance_id = inst_body["id"].as_str().unwrap().to_string();

    // Get the pending timer
    let timers: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/timers"))
        .await
        .json();
    let timer_id = timers[0]["id"].as_str().unwrap().to_string();

    // Fast-forward must succeed (not 500) and complete the instance
    server
        .post(&format!(
            "/v1/process-instances/{instance_id}/timers/{timer_id}/fast-forward"
        ))
        .await
        .assert_status(StatusCode::NO_CONTENT);

    let final_body: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(final_body["state"], "COMPLETED");
}

// ─── Message Events ──────────────────────────────────────────────────────────

const MSG_START_BPMN: &str = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <message id="Msg_NewOrder" name="new-order"/>
  <process id="msg-start-process">
    <startEvent id="start1">
      <outgoing>f1</outgoing>
      <messageEventDefinition messageRef="Msg_NewOrder"/>
    </startEvent>
    <sequenceFlow id="f1" sourceRef="start1" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="task1" targetRef="end1"/>
    <endEvent id="end1"/>
  </process>
</definitions>"#;

const MSG_CATCH_BPMN: &str = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:zeebe="http://camunda.org/schema/zeebe/1.0">
  <message id="Msg_PayReceived" name="payment-received">
    <extensionElements>
      <zeebe:subscription correlationKey="= orderId"/>
    </extensionElements>
  </message>
  <process id="msg-catch-process">
    <startEvent id="start1"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start1" targetRef="catch1"/>
    <intermediateCatchEvent id="catch1">
      <outgoing>f2</outgoing>
      <messageEventDefinition messageRef="Msg_PayReceived"/>
    </intermediateCatchEvent>
    <sequenceFlow id="f2" sourceRef="catch1" targetRef="end1"/>
    <endEvent id="end1"/>
  </process>
</definitions>"#;

const MSG_BOUNDARY_BPMN: &str = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <message id="Msg_Cancel" name="order-cancelled"/>
  <process id="msg-boundary-process">
    <startEvent id="start1"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start1" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="task1" targetRef="end1"/>
    <endEvent id="end1"/>
    <boundaryEvent id="bound1" attachedToRef="task1" cancelActivity="true">
      <outgoing>f3</outgoing>
      <messageEventDefinition messageRef="Msg_Cancel"/>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="bound1" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

const RECEIVE_TASK_BPMN: &str = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:zeebe="http://camunda.org/schema/zeebe/1.0">
  <message id="Msg_Order" name="order-received">
    <extensionElements>
      <zeebe:subscription correlationKey="= orderId"/>
    </extensionElements>
  </message>
  <process id="receive-task-process">
    <startEvent id="start1"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start1" targetRef="rt1"/>
    <receiveTask id="rt1" name="Receive Order" messageRef="Msg_Order">
      <outgoing>f2</outgoing>
    </receiveTask>
    <sequenceFlow id="f2" sourceRef="rt1" targetRef="end1"/>
    <endEvent id="end1"/>
  </process>
</definitions>"#;

const RECEIVE_TASK_BOUNDARY_BPMN: &str = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:zeebe="http://camunda.org/schema/zeebe/1.0">
  <message id="Msg_Order" name="order-received">
    <extensionElements>
      <zeebe:subscription correlationKey="= orderId"/>
    </extensionElements>
  </message>
  <message id="Msg_Cancel" name="order-cancelled">
    <extensionElements>
      <zeebe:subscription correlationKey="= orderId"/>
    </extensionElements>
  </message>
  <process id="receive-task-boundary-process">
    <startEvent id="start1"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start1" targetRef="rt1"/>
    <receiveTask id="rt1" name="Receive Order" messageRef="Msg_Order">
      <outgoing>f2</outgoing>
    </receiveTask>
    <boundaryEvent id="cancel_boundary" attachedToRef="rt1" cancelActivity="true">
      <outgoing>f3</outgoing>
      <messageEventDefinition messageRef="Msg_Cancel"/>
    </boundaryEvent>
    <sequenceFlow id="f2" sourceRef="rt1" targetRef="end_success"/>
    <sequenceFlow id="f3" sourceRef="cancel_boundary" targetRef="end_cancelled"/>
    <endEvent id="end_success"/>
    <endEvent id="end_cancelled"/>
  </process>
</definitions>"#;

/// Deploying a BPMN with a MessageStartEvent and sending a matching message creates a new instance.
#[tokio::test]
async fn message_start_event_creates_instance() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(MSG_START_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let resp: Value = server
        .post("/v1/messages")
        .json(&json!({ "message_name": "new-order", "variables": {} }))
        .await
        .json();

    assert_eq!(resp["instance_state"], "WAITING_FOR_TASK");
    let instance_id = resp["process_instance_id"].as_str().unwrap();

    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_TASK");
}

/// Sending a message to a MessageStartEvent with no matching definition returns 404.
#[tokio::test]
async fn message_no_match_returns_404() {
    let (server, _container) = setup().await;

    server
        .post("/v1/messages")
        .json(&json!({ "message_name": "unknown-message" }))
        .await
        .assert_status(StatusCode::NOT_FOUND);
}

/// MessageIntermediateCatchEvent: correct correlation_key advances the instance to COMPLETED.
#[tokio::test]
async fn message_catch_event_correct_correlation_advances() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(MSG_CATCH_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start instance with orderId = "ord-1"
    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "msg-catch-process", "variables": { "orderId": "ord-1" } }))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_MESSAGE");

    // Send message with correct correlation key
    let resp: Value = server
        .post("/v1/messages")
        .json(&json!({ "message_name": "payment-received", "correlation_key": "ord-1" }))
        .await
        .json();

    assert_eq!(resp["instance_state"], "COMPLETED");
}

/// MessageIntermediateCatchEvent: wrong correlation_key returns 404.
#[tokio::test]
async fn message_catch_event_wrong_correlation_returns_404() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(MSG_CATCH_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start instance with orderId = "ord-1"
    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "msg-catch-process", "variables": { "orderId": "ord-1" } }))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_MESSAGE");

    // Send message with wrong correlation key
    server
        .post("/v1/messages")
        .json(&json!({ "message_name": "payment-received", "correlation_key": "wrong-key" }))
        .await
        .assert_status(StatusCode::NOT_FOUND);
}

/// Interrupting boundary: when the boundary message arrives, the task is cancelled and the boundary path activates.
#[tokio::test]
async fn interrupting_message_boundary_cancels_task() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(MSG_BOUNDARY_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "msg-boundary-process" }))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_TASK");
    let instance_id = inst["id"].as_str().unwrap();

    // Send the boundary message
    let resp: Value = server
        .post("/v1/messages")
        .json(&json!({ "message_name": "order-cancelled" }))
        .await
        .json();

    // Instance should be COMPLETED (boundary path goes directly to end2)
    assert_eq!(resp["instance_state"], "COMPLETED");
    assert_eq!(resp["process_instance_id"], instance_id);
}

/// Message correlation via business_key: the instance was started with a business_key that differs
/// from the subscription expression value; the message must find it via business_key.
#[tokio::test]
async fn message_correlation_by_business_key() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(MSG_CATCH_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Instance A: business_key="bk-A", subscription expr evaluates to "expr-A"
    let inst_a: Value = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "msg-catch-process",
            "business_key": "bk-A",
            "variables": { "orderId": "expr-A" }
        }))
        .await
        .json();
    assert_eq!(inst_a["state"], "WAITING_FOR_MESSAGE");
    assert_eq!(inst_a["business_key"], "bk-A");
    let id_a = inst_a["id"].as_str().unwrap().to_string();

    // Instance B: business_key="bk-B", subscription expr evaluates to "expr-B"
    let inst_b: Value = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "msg-catch-process",
            "business_key": "bk-B",
            "variables": { "orderId": "expr-B" }
        }))
        .await
        .json();
    assert_eq!(inst_b["state"], "WAITING_FOR_MESSAGE");
    let id_b = inst_b["id"].as_str().unwrap().to_string();

    // Send message correlated by business_key "bk-A" (not the subscription expr value "expr-A")
    let resp: Value = server
        .post("/v1/messages")
        .json(&json!({ "message_name": "payment-received", "business_key": "bk-A" }))
        .await
        .json();

    // Instance A wakes up via business_key match
    assert_eq!(resp["process_instance_id"], id_a);
    assert_eq!(resp["instance_state"], "COMPLETED");

    // Instance B is still waiting
    let b: Value = server
        .get(&format!("/v1/process-instances/{id_b}"))
        .await
        .json();
    assert_eq!(b["state"], "WAITING_FOR_MESSAGE");
}

#[tokio::test]
async fn message_ambiguous_correlation_returns_409() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(MSG_CATCH_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start two instances with the SAME business_key
    for _ in 0..2 {
        let inst: Value = server
            .post("/v1/process-instances")
            .json(&json!({
                "process_definition_id": "msg-catch-process",
                "business_key": "duplicate-bk",
                "variables": { "orderId": "unique-expr" }
            }))
            .await
            .json();
        assert_eq!(inst["state"], "WAITING_FOR_MESSAGE");
    }

    // Send message with business_key matching the shared business_key — should 409
    let resp = server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "payment-received",
            "business_key": "duplicate-bk"
        }))
        .await;
    resp.assert_status(StatusCode::CONFLICT);

    let body = resp.text();
    assert!(body.contains("Ambiguous correlation"), "body: {body}");
}

/// Both correlation handles work: one instance wakes via business_key, another via subscription
/// expression value — proving the two paths are independent.
#[tokio::test]
async fn message_both_correlation_handles_work() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(MSG_CATCH_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Instance A: business_key="bk-A", subscription expr → "expr-A"
    let inst_a: Value = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "msg-catch-process",
            "business_key": "bk-A",
            "variables": { "orderId": "expr-A" }
        }))
        .await
        .json();
    assert_eq!(inst_a["state"], "WAITING_FOR_MESSAGE");
    let id_a = inst_a["id"].as_str().unwrap().to_string();

    // Instance B: business_key="bk-B", subscription expr → "expr-B"
    let inst_b: Value = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "msg-catch-process",
            "business_key": "bk-B",
            "variables": { "orderId": "expr-B" }
        }))
        .await
        .json();
    assert_eq!(inst_b["state"], "WAITING_FOR_MESSAGE");
    let id_b = inst_b["id"].as_str().unwrap().to_string();

    // Wake A via business_key (not its subscription expr value "expr-A")
    let resp_a: Value = server
        .post("/v1/messages")
        .json(&json!({ "message_name": "payment-received", "business_key": "bk-A" }))
        .await
        .json();
    assert_eq!(resp_a["process_instance_id"], id_a);
    assert_eq!(resp_a["instance_state"], "COMPLETED");

    // Wake B via subscription expression value (not its business_key "bk-B")
    let resp_b: Value = server
        .post("/v1/messages")
        .json(&json!({ "message_name": "payment-received", "correlation_key": "expr-B" }))
        .await
        .json();
    assert_eq!(resp_b["process_instance_id"], id_b);
    assert_eq!(resp_b["instance_state"], "COMPLETED");
}

/// Direct instance targeting via process_instance_id bypasses correlation matching entirely.
#[tokio::test]
async fn message_correlation_by_process_instance_id() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(MSG_CATCH_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start two instances — both waiting for same message with different correlation keys
    let inst_a: Value = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "msg-catch-process",
            "variables": { "orderId": "expr-A" }
        }))
        .await
        .json();
    assert_eq!(inst_a["state"], "WAITING_FOR_MESSAGE");
    let id_a = inst_a["id"].as_str().unwrap().to_string();

    let inst_b: Value = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "msg-catch-process",
            "variables": { "orderId": "expr-B" }
        }))
        .await
        .json();
    assert_eq!(inst_b["state"], "WAITING_FOR_MESSAGE");
    let id_b = inst_b["id"].as_str().unwrap().to_string();

    // Target instance B directly — no correlation_key needed
    let resp: Value = server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "payment-received",
            "process_instance_id": id_b
        }))
        .await
        .json();
    assert_eq!(resp["process_instance_id"], id_b);
    assert_eq!(resp["instance_state"], "COMPLETED");

    // Instance A still waiting
    let a: Value = server
        .get(&format!("/v1/process-instances/{id_a}"))
        .await
        .json();
    assert_eq!(a["state"], "WAITING_FOR_MESSAGE");
}

/// Combined filters: business_key + correlation_key must both match to find the subscription.
#[tokio::test]
async fn message_correlation_combined_filters() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(MSG_CATCH_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Instance with business_key="bk-X" and subscription expr → "expr-X"
    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "msg-catch-process",
            "business_key": "bk-X",
            "variables": { "orderId": "expr-X" }
        }))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_MESSAGE");
    let id = inst["id"].as_str().unwrap().to_string();

    // Wrong business_key + right correlation_key → no match
    let resp = server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "payment-received",
            "correlation_key": "expr-X",
            "business_key": "wrong-bk"
        }))
        .await;
    resp.assert_status_not_found();

    // Right business_key + wrong correlation_key → no match
    let resp = server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "payment-received",
            "correlation_key": "wrong-expr",
            "business_key": "bk-X"
        }))
        .await;
    resp.assert_status_not_found();

    // Both correct → match
    let resp: Value = server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "payment-received",
            "correlation_key": "expr-X",
            "business_key": "bk-X"
        }))
        .await
        .json();
    assert_eq!(resp["process_instance_id"], id);
    assert_eq!(resp["instance_state"], "COMPLETED");
}

/// Targeting a non-existent process_instance_id returns 404.
#[tokio::test]
async fn message_process_instance_id_not_found() {
    let (server, _container) = setup().await;

    let resp = server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "payment-received",
            "process_instance_id": "nonexistent-id"
        }))
        .await;
    resp.assert_status_not_found();
}

#[tokio::test]
async fn receive_task_basic() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(RECEIVE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "receive-task-process",
            "variables": { "orderId": "order-42" }
        }))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_MESSAGE");
    let id = inst["id"].as_str().unwrap().to_string();

    // Send the matching message
    let resp: Value = server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "order-received",
            "correlation_key": "order-42",
            "variables": { "confirmed": true }
        }))
        .await
        .json();
    assert_eq!(resp["process_instance_id"], id);
    assert_eq!(resp["instance_state"], "COMPLETED");

    // Verify message variables were merged into instance
    let final_inst: Value = server
        .get(&format!("/v1/process-instances/{id}"))
        .await
        .json();
    assert_eq!(final_inst["variables"]["confirmed"], true);
}

#[tokio::test]
async fn receive_task_wrong_correlation_key_does_not_match() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(RECEIVE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "receive-task-process",
            "variables": { "orderId": "order-A" }
        }))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_MESSAGE");

    // Send with wrong correlation key — should 404
    server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "order-received",
            "correlation_key": "order-B"
        }))
        .await
        .assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn receive_task_interrupting_message_boundary() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(RECEIVE_TASK_BOUNDARY_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "receive-task-boundary-process",
            "variables": { "orderId": "order-99" }
        }))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_MESSAGE");
    let id = inst["id"].as_str().unwrap().to_string();

    // Fire the CANCEL boundary message instead of the body message
    let resp: Value = server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "order-cancelled",
            "correlation_key": "order-99"
        }))
        .await
        .json();
    assert_eq!(resp["process_instance_id"], id);
    assert_eq!(resp["instance_state"], "COMPLETED");

    // Body message subscription was cancelled — sending it now should 404
    server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "order-received",
            "correlation_key": "order-99"
        }))
        .await
        .assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn receive_task_body_cancels_boundary_subscription() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(RECEIVE_TASK_BOUNDARY_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "receive-task-boundary-process",
            "variables": { "orderId": "order-77" }
        }))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_MESSAGE");
    let id = inst["id"].as_str().unwrap().to_string();

    // Fire the body message
    let resp: Value = server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "order-received",
            "correlation_key": "order-77"
        }))
        .await
        .json();
    assert_eq!(resp["instance_state"], "COMPLETED");
    let _ = id;

    // Boundary subscription was consumed — sending cancel now should 404
    server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "order-cancelled",
            "correlation_key": "order-77"
        }))
        .await
        .assert_status(StatusCode::NOT_FOUND);
}

const PAG_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="pag">
  <process id="pag-process" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="end"/>
    <endEvent id="end"></endEvent>
  </process>
</definitions>"#;

#[tokio::test]
async fn test_list_instances_paginated() {
    let (server, _container) = setup().await;

    // Deploy definition
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(PAG_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start 5 instances
    for _ in 0..5 {
        server
            .post("/v1/process-instances")
            .json(&json!({ "process_definition_id": "pag-process" }))
            .await
            .assert_status(StatusCode::CREATED);
    }

    // Page 1 with page_size=2
    let body: Value = server
        .get("/v1/process-instances?definition_id=pag-process&page=1&page_size=2")
        .await
        .json();
    assert_eq!(body["total"], 5, "total should be 5");
    assert_eq!(body["page"], 1);
    assert_eq!(body["page_size"], 2);
    assert_eq!(body["total_pages"], 3);
    assert_eq!(body["items"].as_array().unwrap().len(), 2);

    // Page 3 with page_size=2 → only 1 item
    let body: Value = server
        .get("/v1/process-instances?definition_id=pag-process&page=3&page_size=2")
        .await
        .json();
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
}

const VERSIONS_BPMN_V1: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="ver">
  <process id="ver-process" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="end"/>
    <endEvent id="end"></endEvent>
  </process>
</definitions>"#;

const VERSIONS_BPMN_V2: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="ver2">
  <process id="ver-process" isExecutable="true">
    <startEvent id="start2"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start2" targetRef="end2"/>
    <endEvent id="end2"></endEvent>
  </process>
</definitions>"#;

#[tokio::test]
async fn test_list_definition_versions() {
    let (server, _container) = setup().await;

    // Deploy v1
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(VERSIONS_BPMN_V1)
        .await
        .assert_status(StatusCode::CREATED);

    // Before deploying v2, versions endpoint returns [1] with latest=1
    let body: Value = server
        .get("/v1/process-definitions/ver-process/versions")
        .await
        .json();
    assert_eq!(body["latest"], 1);
    assert_eq!(body["versions"].as_array().unwrap().len(), 1);

    // Deploy v2
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(VERSIONS_BPMN_V2)
        .await
        .assert_status(StatusCode::CREATED);

    // Now versions endpoint returns [2, 1] newest-first with latest=2
    let body: Value = server
        .get("/v1/process-definitions/ver-process/versions")
        .await
        .json();
    assert_eq!(body["latest"], 2);
    let versions = body["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0], 2); // newest first
    assert_eq!(versions[1], 1);

    // Unknown definition → 404
    server
        .get("/v1/process-definitions/no-such-process/versions")
        .await
        .assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_list_instances_filtered_by_version() {
    let (server, _container) = setup().await;

    // Deploy v1 then v2 of the same process
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(VERSIONS_BPMN_V1)
        .await
        .assert_status(StatusCode::CREATED);
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(VERSIONS_BPMN_V2)
        .await
        .assert_status(StatusCode::CREATED);

    // Start 2 instances of v1 (explicit version suffix)
    for _ in 0..2 {
        server
            .post("/v1/process-instances")
            .json(&json!({ "process_definition_id": "ver-process:1" }))
            .await
            .assert_status(StatusCode::CREATED);
    }

    // Start 3 instances of v2 (latest)
    for _ in 0..3 {
        server
            .post("/v1/process-instances")
            .json(&json!({ "process_definition_id": "ver-process" }))
            .await
            .assert_status(StatusCode::CREATED);
    }

    // Without version filter: all 5 instances
    let body: Value = server
        .get("/v1/process-instances?definition_id=ver-process")
        .await
        .json();
    assert_eq!(body["total"], 5);

    // Filter to version 1: only 2 instances
    let body: Value = server
        .get("/v1/process-instances?definition_id=ver-process&version=1")
        .await
        .json();
    assert_eq!(body["total"], 2);
    for item in body["items"].as_array().unwrap() {
        assert_eq!(item["process_definition_id"], "ver-process");
    }

    // Filter to version 2: only 3 instances
    let body: Value = server
        .get("/v1/process-instances?definition_id=ver-process&version=2")
        .await
        .json();
    assert_eq!(body["total"], 3);
}

/// Starting with an explicit ":N" suffix targets that specific version.
/// The response must carry process_definition_version == N.
#[tokio::test]
async fn test_start_instance_explicit_version_targets_that_version() {
    let (server, _container) = setup().await;

    // Deploy v1 then v2
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(VERSIONS_BPMN_V1)
        .await
        .assert_status(StatusCode::CREATED);
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(VERSIONS_BPMN_V2)
        .await
        .assert_status(StatusCode::CREATED);

    // Start against v1 explicitly
    let resp = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "ver-process:1" }))
        .await;
    resp.assert_status(StatusCode::CREATED);
    let body: Value = resp.json();
    assert_eq!(body["process_definition_id"], "ver-process");
    assert_eq!(body["process_definition_version"], 1);

    // Start against v2 explicitly
    let resp = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "ver-process:2" }))
        .await;
    resp.assert_status(StatusCode::CREATED);
    let body: Value = resp.json();
    assert_eq!(body["process_definition_id"], "ver-process");
    assert_eq!(body["process_definition_version"], 2);
}

/// Starting without a version suffix targets the latest deployed version.
#[tokio::test]
async fn test_start_instance_without_version_targets_latest() {
    let (server, _container) = setup().await;

    // Deploy v1
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(VERSIONS_BPMN_V1)
        .await
        .assert_status(StatusCode::CREATED);

    // Without suffix → v1 (latest so far)
    let resp = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "ver-process" }))
        .await;
    resp.assert_status(StatusCode::CREATED);
    let body: Value = resp.json();
    assert_eq!(body["process_definition_version"], 1);

    // Deploy v2 — now latest is v2
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(VERSIONS_BPMN_V2)
        .await
        .assert_status(StatusCode::CREATED);

    // Without suffix → v2 (new latest)
    let resp = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "ver-process" }))
        .await;
    resp.assert_status(StatusCode::CREATED);
    let body: Value = resp.json();
    assert_eq!(body["process_definition_version"], 2);
}

// ── Receive task: <message> defined AFTER </process> (bpmn.io default) ───────

/// Regression: bpmn.io places <message> elements after </process>.
/// The parser must still resolve messageRef → human-readable name so that
/// POST /v1/messages with `message_name` matches the subscription.
const RECEIVE_TASK_MSG_AFTER_PROCESS_BPMN: &str = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:zeebe="http://camunda.org/schema/zeebe/1.0">
  <process id="rt-msg-after-process">
    <startEvent id="s1"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s1" targetRef="rt1"/>
    <receiveTask id="rt1" name="Wait for Payment" messageRef="Msg_Pay">
      <outgoing>f2</outgoing>
    </receiveTask>
    <sequenceFlow id="f2" sourceRef="rt1" targetRef="e1"/>
    <endEvent id="e1"/>
  </process>
  <message id="Msg_Pay" name="payment-received">
    <extensionElements>
      <zeebe:subscription correlationKey="= orderId"/>
    </extensionElements>
  </message>
</definitions>"#;

#[tokio::test]
async fn receive_task_message_defined_after_process() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(RECEIVE_TASK_MSG_AFTER_PROCESS_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start instance — should land on WAITING_FOR_MESSAGE
    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "rt-msg-after-process",
            "variables": { "orderId": "pay-77" }
        }))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_MESSAGE");
    let id = inst["id"].as_str().unwrap().to_string();

    // Send message using the human-readable name — must match
    let resp: Value = server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "payment-received",
            "correlation_key": "pay-77",
            "variables": { "paid": true }
        }))
        .await
        .json();
    assert_eq!(resp["process_instance_id"], id);
    assert_eq!(resp["instance_state"], "COMPLETED");

    // Verify message variables were merged
    let final_inst: Value = server
        .get(&format!("/v1/process-instances/{id}"))
        .await
        .json();
    assert_eq!(final_inst["variables"]["paid"], true);
}

#[tokio::test]
async fn test_script_task_executes_inline() {
    let (server, _container) = setup().await;

    let bpmn = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:zeebe="http://camunda.org/schema/zeebe/1.0">
  <process id="ScriptProcess" name="Script Test" isExecutable="true">
    <startEvent id="Start_1">
      <outgoing>Flow_1</outgoing>
    </startEvent>
    <scriptTask id="Script_1" name="Calculate" scriptFormat="rhai">
      <incoming>Flow_1</incoming>
      <outgoing>Flow_2</outgoing>
      <script>let result = price * quantity; result</script>
      <extensionElements>
        <zeebe:script resultVariable="total" />
      </extensionElements>
    </scriptTask>
    <endEvent id="End_1">
      <incoming>Flow_2</incoming>
    </endEvent>
    <sequenceFlow id="Flow_1" sourceRef="Start_1" targetRef="Script_1" />
    <sequenceFlow id="Flow_2" sourceRef="Script_1" targetRef="End_1" />
  </process>
</definitions>"#;

    // Deploy
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(bpmn)
        .await
        .assert_status(StatusCode::CREATED);

    // Start with variables
    let response = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "ScriptProcess",
            "variables": { "price": 25, "quantity": 4 }
        }))
        .await;
    response.assert_status(StatusCode::CREATED);
    let body: Value = response.json();
    let instance_id = body["id"].as_str().unwrap();

    // Verify completed with correct variables
    let resp = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["state"], "COMPLETED");
    assert_eq!(body["variables"]["total"], 100);
    assert_eq!(body["variables"]["price"], 25);
    assert_eq!(body["variables"]["quantity"], 4);
}

// ── Instance retry tests ──────────────────────────────────────────────────────

/// BPMN: start → timer → exclusive gateway (condition: ${status} == 'approved') → task_a
///       gateway also has default flow → task_b
/// Timer fires into gateway — if status is missing, the gateway takes the default flow.
/// If no default flow, gateway fails.
const TIMER_GATEWAY_NO_DEFAULT_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="tg">
  <process id="timer-gw-process" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="timer1"/>
    <intermediateCatchEvent id="timer1">
      <outgoing>sf2</outgoing>
      <timerEventDefinition><timeDuration>PT1S</timeDuration></timerEventDefinition>
    </intermediateCatchEvent>
    <sequenceFlow id="sf2" sourceRef="timer1" targetRef="gw1"/>
    <exclusiveGateway id="gw1">
      <outgoing>sf3</outgoing>
    </exclusiveGateway>
    <sequenceFlow id="sf3" sourceRef="gw1" targetRef="task_a">
      <conditionExpression>${status} == 'approved'</conditionExpression>
    </sequenceFlow>
    <serviceTask id="task_a"><outgoing>sf4</outgoing></serviceTask>
    <sequenceFlow id="sf4" sourceRef="task_a" targetRef="end"/>
    <endEvent id="end"></endEvent>
  </process>
</definitions>"#;

/// Timer fires into an exclusive gateway whose condition doesn't match and there's
/// no default flow → instance should become FAILED with an error message.
/// Then retry after fixing variables → instance should recover.
#[tokio::test]
async fn retry_failed_instance_after_gateway_error() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(TIMER_GATEWAY_NO_DEFAULT_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start instance without the 'status' variable → gateway will fail
    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "timer-gw-process" }))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_TIMER");
    let instance_id = inst["id"].as_str().unwrap().to_string();

    // Get the timer and fast-forward it
    let timers: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/timers"))
        .await
        .json();
    let timer_id = timers[0]["id"].as_str().unwrap().to_string();

    // Fast-forward — this will fire the timer which hits the gateway and fails
    let ff_resp = server
        .post(&format!(
            "/v1/process-instances/{instance_id}/timers/{timer_id}/fast-forward"
        ))
        .await;
    assert_eq!(ff_resp.status_code(), StatusCode::INTERNAL_SERVER_ERROR);

    // Instance should now be FAILED with an error message
    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "FAILED");
    assert!(inst["error_message"]
        .as_str()
        .unwrap()
        .contains("no outgoing flow condition matched"));

    // Fix variables — set 'status' so the gateway condition passes
    server
        .put(&format!("/v1/process-instances/{instance_id}/variables"))
        .json(&json!({ "variables": { "status": "approved" } }))
        .await
        .assert_status_ok();

    // Retry the instance
    let retry_resp = server
        .post(&format!("/v1/process-instances/{instance_id}/retry"))
        .await;
    assert_eq!(retry_resp.status_code(), StatusCode::OK);
    let retried: Value = retry_resp.json();
    assert_eq!(retried["state"], "WAITING_FOR_TIMER");
    assert!(retried["error_message"].is_null());
    assert!(retried["ended_at"].is_null());

    // Should have a new timer (side effect re-created)
    let timers: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/timers"))
        .await
        .json();
    let unfired: Vec<&Value> = timers
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["fired"].as_bool() == Some(false))
        .collect();
    assert_eq!(unfired.len(), 1, "expected 1 unfired timer after retry");

    // Fast-forward the new timer — now gateway should succeed
    let new_timer_id = unfired[0]["id"].as_str().unwrap().to_string();
    server
        .post(&format!(
            "/v1/process-instances/{instance_id}/timers/{new_timer_id}/fast-forward"
        ))
        .await
        .assert_status(StatusCode::NO_CONTENT);

    // Instance should now be at task_a
    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_TASK");
    assert_eq!(inst["active_element_ids"], json!(["task_a"]));
}

/// Retrying a non-FAILED instance returns 409 Conflict.
#[tokio::test]
async fn retry_non_failed_instance_returns_409() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(SERVICE_TASK_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "svc-process" }))
        .await
        .json();
    let instance_id = inst["id"].as_str().unwrap().to_string();

    // Instance is WAITING_FOR_TASK — retry should return 409
    server
        .post(&format!("/v1/process-instances/{instance_id}/retry"))
        .await
        .assert_status(StatusCode::CONFLICT);
}

/// Retrying a non-existent instance returns 404.
#[tokio::test]
async fn retry_nonexistent_instance_returns_404() {
    let (server, _container) = setup().await;

    server
        .post(&format!("/v1/process-instances/{}/retry", Uuid::new_v4()))
        .await
        .assert_status(StatusCode::NOT_FOUND);
}

// ── Cancel subscription cleanup tests ─────────────────────────────────────────

/// Cancel should clean up open message subscriptions.
#[tokio::test]
async fn cancel_instance_cleans_up_message_subscriptions() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(MESSAGE_CATCH_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({
            "process_definition_id": "msg-process",
            "variables": { "order_id": "ord-cleanup-test" }
        }))
        .await
        .json();
    let instance_id = inst["id"].as_str().unwrap().to_string();
    assert_eq!(inst["state"], "WAITING_FOR_MESSAGE");

    // Cancel the instance
    server
        .post(&format!("/v1/process-instances/{instance_id}/cancel"))
        .await
        .assert_status_ok();

    // Try to send a message — should NOT match because subscription was cleaned up
    let msg_resp = server
        .post("/v1/messages")
        .json(&json!({
            "message_name": "OrderApproved",
            "correlation_key": "ord-cleanup-test"
        }))
        .await;
    // Should return 404 (no matching subscription) rather than 200
    assert_eq!(msg_resp.status_code(), StatusCode::NOT_FOUND);
}

/// Cancel should mark unfired timers as fired.
#[tokio::test]
async fn cancel_instance_cleans_up_timers() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(TIMER_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "timer-process" }))
        .await
        .json();
    let instance_id = inst["id"].as_str().unwrap().to_string();
    assert_eq!(inst["state"], "WAITING_FOR_TIMER");

    // Verify there's an unfired timer
    let timers: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/timers"))
        .await
        .json();
    assert_eq!(timers.as_array().unwrap().len(), 1);
    assert_eq!(timers[0]["fired"], false);

    // Cancel the instance
    server
        .post(&format!("/v1/process-instances/{instance_id}/cancel"))
        .await
        .assert_status_ok();

    // Timer should now be marked as fired
    let timers: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/timers"))
        .await
        .json();
    assert_eq!(timers[0]["fired"], true);
}

const TIMER_BOUNDARY_BPMN: &str = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="timer-boundary-process">
    <startEvent id="start1"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start1" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="task1" targetRef="end1"/>
    <endEvent id="end1"/>
    <boundaryEvent id="timer_bound" attachedToRef="task1" cancelActivity="true">
      <outgoing>f3</outgoing>
      <timerEventDefinition><timeDuration>PT1S</timeDuration></timerEventDefinition>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="timer_bound" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

#[tokio::test]
async fn timer_boundary_interrupting_fires_and_cancels_task() {
    let (server, _container) = setup().await;

    // Deploy
    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(TIMER_BOUNDARY_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    // Start — pauses at task1
    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "timer-boundary-process" }))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_TASK");
    let instance_id = inst["id"].as_str().unwrap();

    // Verify timer was scheduled for the boundary event
    let timers: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/timers"))
        .await
        .json();
    assert_eq!(timers.as_array().unwrap().len(), 1);
    assert_eq!(timers[0]["element_id"], "timer_bound");

    // Fast-forward the timer
    let timer_id = timers[0]["id"].as_str().unwrap();
    server
        .post(&format!(
            "/v1/process-instances/{instance_id}/timers/{timer_id}/fast-forward"
        ))
        .await
        .assert_status(StatusCode::NO_CONTENT);

    // Verify instance completed via boundary path
    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "COMPLETED");
}

#[tokio::test]
async fn timer_boundary_cancelled_when_task_completes_normally() {
    let (server, _container) = setup().await;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(TIMER_BOUNDARY_BPMN)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "timer-boundary-process" }))
        .await
        .json();
    let instance_id = inst["id"].as_str().unwrap();

    // Claim and complete task normally
    let tasks: Value = server.get("/v1/tasks").await.json();
    let task_id = tasks[0]["id"].as_str().unwrap();
    server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({ "claimed_by": "worker-1" }))
        .await
        .assert_status_ok();
    server
        .post(&format!("/v1/tasks/{task_id}/complete"))
        .json(&json!({}))
        .await
        .assert_status_success();

    // Verify instance completed via normal path AND timer was cancelled
    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "COMPLETED");

    let timers: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/timers"))
        .await
        .json();
    // Timer should be marked as fired (cancelled)
    assert_eq!(timers[0]["fired"], true);
}

#[tokio::test]
async fn timer_boundary_non_interrupting_keeps_task_active() {
    let (server, _container) = setup().await;

    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="timer-nonint-process">
    <startEvent id="start1"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start1" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="task1" targetRef="end1"/>
    <endEvent id="end1"/>
    <boundaryEvent id="timer_bound" attachedToRef="task1" cancelActivity="false">
      <outgoing>f3</outgoing>
      <timerEventDefinition><timeDuration>PT1S</timeDuration></timerEventDefinition>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="timer_bound" targetRef="task2"/>
    <serviceTask id="task2"><outgoing>f4</outgoing></serviceTask>
    <sequenceFlow id="f4" sourceRef="task2" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(xml)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "timer-nonint-process" }))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_TASK");
    let instance_id = inst["id"].as_str().unwrap();

    // Fast-forward the boundary timer (non-interrupting)
    let timers: Value = server
        .get(&format!("/v1/process-instances/{instance_id}/timers"))
        .await
        .json();
    assert_eq!(timers.as_array().unwrap().len(), 1);
    let timer_id = timers[0]["id"].as_str().unwrap();
    server
        .post(&format!(
            "/v1/process-instances/{instance_id}/timers/{timer_id}/fast-forward"
        ))
        .await
        .assert_status(StatusCode::NO_CONTENT);

    // Instance should still be running — task1 AND task2 both active
    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_TASK");

    // Should have two tasks now
    let tasks: Value = server.get("/v1/tasks").await.json();
    let instance_tasks: Vec<&Value> = tasks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["process_instance_id"].as_str() == Some(instance_id))
        .collect();
    assert_eq!(
        instance_tasks.len(),
        2,
        "expected task1 and task2 both active"
    );
}

// ============================================================
// Signal events
// ============================================================

#[tokio::test]
async fn signal_start_event_creates_instance_on_broadcast() {
    let (server, _container) = setup().await;

    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <signal id="Sig_1" name="OrderPlaced"/>
  <process id="sig-start-process">
    <startEvent id="sig_start">
      <signalEventDefinition signalRef="Sig_1"/>
      <outgoing>f1</outgoing>
    </startEvent>
    <sequenceFlow id="f1" sourceRef="sig_start" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(xml)
        .await
        .assert_status(StatusCode::CREATED);

    // Broadcast signal — should create a new instance
    let resp: Value = server
        .post("/v1/signals/OrderPlaced")
        .json(&json!({ "variables": { "order_id": "ORD-123" } }))
        .await
        .json();

    assert!(resp["started_count"].as_i64().unwrap() >= 1);

    // Verify the instance was created and has the injected variable
    let instances: Value = server.get("/v1/process-instances").await.json();
    let inst = instances["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["process_definition_id"] == "sig-start-process")
        .expect("instance should have been created");
    assert_eq!(inst["state"], "WAITING_FOR_TASK");
    assert_eq!(inst["variables"]["order_id"], "ORD-123");
}

#[tokio::test]
async fn signal_boundary_interrupting_cancels_task_on_broadcast() {
    let (server, _container) = setup().await;

    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <signal id="Sig_1" name="CancelOrder"/>
  <process id="sig-boundary-process">
    <startEvent id="start1"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start1" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="task1" targetRef="end1"/>
    <endEvent id="end1"/>
    <boundaryEvent id="sig_bound" attachedToRef="task1" cancelActivity="true">
      <outgoing>f3</outgoing>
      <signalEventDefinition signalRef="Sig_1"/>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="sig_bound" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(xml)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "sig-boundary-process" }))
        .await
        .json();
    assert_eq!(inst["state"], "WAITING_FOR_TASK");
    let instance_id = inst["id"].as_str().unwrap();

    // Broadcast cancel signal — should cancel task, complete via boundary
    server
        .post("/v1/signals/CancelOrder")
        .json(&json!({}))
        .await
        .assert_status_success();

    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "COMPLETED");
}

#[tokio::test]
async fn signal_boundary_cancelled_when_task_completes_normally() {
    let (server, _container) = setup().await;

    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <signal id="Sig_1" name="CancelOrder"/>
  <process id="sig-boundary-cancel-process">
    <startEvent id="start1"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start1" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="task1" targetRef="end1"/>
    <endEvent id="end1"/>
    <boundaryEvent id="sig_bound" attachedToRef="task1" cancelActivity="true">
      <outgoing>f3</outgoing>
      <signalEventDefinition signalRef="Sig_1"/>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="sig_bound" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(xml)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "sig-boundary-cancel-process" }))
        .await
        .json();
    let instance_id = inst["id"].as_str().unwrap();

    // Claim and complete task normally
    let tasks: Value = server.get("/v1/tasks").await.json();
    let task_id = tasks[0]["id"].as_str().unwrap();
    server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({ "claimed_by": "test-worker" }))
        .await
        .assert_status_success();
    server
        .post(&format!("/v1/tasks/{task_id}/complete"))
        .json(&json!({}))
        .await
        .assert_status_success();

    // Instance completed via normal path
    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "COMPLETED");

    // Signal should NOT match anything now (subscription consumed)
    let resp: Value = server
        .post("/v1/signals/CancelOrder")
        .json(&json!({}))
        .await
        .json();
    assert_eq!(resp["woken_count"], 0);
}

// ---- Error End Event tests ----

#[tokio::test]
async fn error_end_event_in_subprocess_caught_by_boundary() {
    let (server, _container) = setup().await;

    // Subprocess immediately hits ErrorEndEvent → error boundary catches it → COMPLETED
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <error id="Err_1" name="PaymentFailed" errorCode="PAYMENT_ERR"/>
  <process id="error-subprocess-process">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="sub"/>
    <subProcess id="sub">
      <startEvent id="sub_s"><outgoing>sf1</outgoing></startEvent>
      <sequenceFlow id="sf1" sourceRef="sub_s" targetRef="err_end"/>
      <endEvent id="err_end">
        <errorEventDefinition errorRef="Err_1"/>
      </endEvent>
      <outgoing>f2</outgoing>
    </subProcess>
    <sequenceFlow id="f2" sourceRef="sub" targetRef="end1"/>
    <endEvent id="end1"/>
    <boundaryEvent id="err_bound" attachedToRef="sub">
      <outgoing>f3</outgoing>
      <errorEventDefinition errorRef="Err_1"/>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="err_bound" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(xml)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "error-subprocess-process" }))
        .await
        .json();

    // Should complete via error boundary path, not FAILED
    assert_eq!(inst["state"], "COMPLETED");
}

#[tokio::test]
async fn terminate_end_cancels_parallel_task_and_completes() {
    let (server, _container) = setup().await;

    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="term-process">
    <startEvent id="start"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start" targetRef="fork"/>
    <parallelGateway id="fork"><incoming>f1</incoming><outgoing>f2</outgoing><outgoing>f3</outgoing></parallelGateway>
    <sequenceFlow id="f2" sourceRef="fork" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f4</outgoing></serviceTask>
    <sequenceFlow id="f4" sourceRef="task1" targetRef="end1"/>
    <endEvent id="end1"/>
    <sequenceFlow id="f3" sourceRef="fork" targetRef="term"/>
    <endEvent id="term"><terminateEventDefinition/></endEvent>
  </process>
</definitions>"#;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(xml)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "term-process" }))
        .await
        .json();

    // Terminate fires immediately — instance should be COMPLETED, no tasks created
    assert_eq!(inst["state"], "COMPLETED");

    // No tasks should be pending for this instance
    let tasks: Value = server.get("/v1/tasks").await.json();
    let pending: Vec<_> = tasks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["process_instance_id"] == inst["id"])
        .collect();
    assert_eq!(
        pending.len(),
        0,
        "No tasks should exist after terminate end event"
    );
}

#[tokio::test]
async fn terminate_end_after_task_completion_cancels_sibling_tasks() {
    let (server, _container) = setup().await;

    // Two parallel branches: task_a → terminate, task_b → normal end
    // Completing task_a reaches terminate, should cancel task_b
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="term-after-task">
    <startEvent id="start"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start" targetRef="fork"/>
    <parallelGateway id="fork"><incoming>f1</incoming><outgoing>f2</outgoing><outgoing>f3</outgoing></parallelGateway>
    <sequenceFlow id="f2" sourceRef="fork" targetRef="task_a"/>
    <serviceTask id="task_a"><outgoing>f4</outgoing></serviceTask>
    <sequenceFlow id="f4" sourceRef="task_a" targetRef="term"/>
    <endEvent id="term"><terminateEventDefinition/></endEvent>
    <sequenceFlow id="f3" sourceRef="fork" targetRef="task_b"/>
    <serviceTask id="task_b"><outgoing>f5</outgoing></serviceTask>
    <sequenceFlow id="f5" sourceRef="task_b" targetRef="end1"/>
    <endEvent id="end1"/>
  </process>
</definitions>"#;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(xml)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "term-after-task" }))
        .await
        .json();

    // Both tasks should be created
    assert_eq!(inst["state"], "WAITING_FOR_TASK");
    let inst_id = inst["id"].as_str().unwrap();

    let tasks: Value = server.get("/v1/tasks").await.json();
    let inst_tasks: Vec<&Value> = tasks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["process_instance_id"] == inst_id)
        .collect();
    assert_eq!(
        inst_tasks.len(),
        2,
        "Should have 2 tasks after parallel fork"
    );

    // Claim and complete task_a → reaches terminate
    let task_a = inst_tasks
        .iter()
        .find(|t| t["element_id"] == "task_a")
        .unwrap();
    let task_a_id = task_a["id"].as_str().unwrap();
    server
        .post(&format!("/v1/tasks/{task_a_id}/claim"))
        .json(&json!({"claimed_by": "test"}))
        .await
        .assert_status(StatusCode::OK);
    server
        .post(&format!("/v1/tasks/{task_a_id}/complete"))
        .json(&json!({}))
        .await
        .assert_status(StatusCode::OK);

    // Instance should be COMPLETED
    let inst_after: Value = server
        .get(&format!("/v1/process-instances/{inst_id}"))
        .await
        .json();
    assert_eq!(
        inst_after["state"], "COMPLETED",
        "Instance should be COMPLETED after terminate"
    );

    // task_b should be CANCELLED (not still CREATED)
    let tasks_after: Value = server.get("/v1/tasks").await.json();
    let remaining: Vec<&Value> = tasks_after
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["process_instance_id"] == inst_id && t["state"] == "CREATED")
        .collect();
    assert_eq!(
        remaining.len(),
        0,
        "No tasks should remain CREATED after terminate"
    );
}

#[tokio::test]
async fn escalation_boundary_catches_subprocess_escalation() {
    let (server, _container) = setup().await;

    // Subprocess immediately throws escalation → interrupting boundary catches it → routes to esc_task
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <escalation id="esc1" escalationCode="ESC_001"/>
  <process id="esc-boundary-test">
    <startEvent id="start"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start" targetRef="sub"/>
    <subProcess id="sub"><outgoing>f2</outgoing>
      <startEvent id="sub_s"><outgoing>sf1</outgoing></startEvent>
      <sequenceFlow id="sf1" sourceRef="sub_s" targetRef="sub_esc_end"/>
      <endEvent id="sub_esc_end">
        <escalationEventDefinition escalationRef="esc1"/>
      </endEvent>
    </subProcess>
    <sequenceFlow id="f2" sourceRef="sub" targetRef="normal_end"/>
    <endEvent id="normal_end"/>
    <boundaryEvent id="esc_bound" attachedToRef="sub" cancelActivity="true">
      <outgoing>f3</outgoing>
      <escalationEventDefinition escalationRef="esc1"/>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="esc_bound" targetRef="esc_task"/>
    <serviceTask id="esc_task"><outgoing>f4</outgoing></serviceTask>
    <sequenceFlow id="f4" sourceRef="esc_task" targetRef="esc_end"/>
    <endEvent id="esc_end"/>
  </process>
</definitions>"#;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(xml)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "esc-boundary-test" }))
        .await
        .json();

    // Boundary caught the escalation → waiting on esc_task
    assert_eq!(inst["state"], "WAITING_FOR_TASK");

    let tasks: Value = server.get("/v1/tasks").await.json();
    let inst_id = inst["id"].as_str().unwrap();
    let inst_tasks: Vec<&Value> = tasks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["process_instance_id"] == inst_id)
        .collect();
    assert_eq!(inst_tasks.len(), 1);
    assert_eq!(inst_tasks[0]["element_id"], "esc_task");
}

#[tokio::test]
async fn link_throw_jumps_to_catch_in_server() {
    let (server, _container) = setup().await;

    // start → link_throw(jump1) → link_catch(jump1) → task1 → end
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="link-test">
    <startEvent id="start"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start" targetRef="link_throw"/>
    <intermediateThrowEvent id="link_throw">
      <linkEventDefinition name="jump1"/>
    </intermediateThrowEvent>
    <intermediateCatchEvent id="link_catch">
      <linkEventDefinition name="jump1"/>
      <outgoing>f2</outgoing>
    </intermediateCatchEvent>
    <sequenceFlow id="f2" sourceRef="link_catch" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f3</outgoing></serviceTask>
    <sequenceFlow id="f3" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(xml)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "link-test" }))
        .await
        .json();

    // Link throw jumps to catch → waiting on task1
    assert_eq!(inst["state"], "WAITING_FOR_TASK");

    let tasks: Value = server.get("/v1/tasks").await.json();
    let inst_id = inst["id"].as_str().unwrap();
    let inst_tasks: Vec<&Value> = tasks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["process_instance_id"] == inst_id)
        .collect();
    assert_eq!(inst_tasks.len(), 1);
    assert_eq!(inst_tasks[0]["element_id"], "task1");
}

#[tokio::test]
async fn terminate_end_cancels_parallel_timer() {
    let (server, _container) = setup().await;

    // Parallel: branch A → task_a → terminate, branch B → timer → end
    // Completing task_a triggers terminate → timer on branch B should be cancelled
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="term-timer">
    <startEvent id="start"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start" targetRef="fork"/>
    <parallelGateway id="fork"><incoming>f1</incoming><outgoing>f2</outgoing><outgoing>f3</outgoing></parallelGateway>
    <sequenceFlow id="f2" sourceRef="fork" targetRef="task_a"/>
    <serviceTask id="task_a"><outgoing>f4</outgoing></serviceTask>
    <sequenceFlow id="f4" sourceRef="task_a" targetRef="term"/>
    <endEvent id="term"><terminateEventDefinition/></endEvent>
    <sequenceFlow id="f3" sourceRef="fork" targetRef="timer1"/>
    <intermediateThrowEvent id="timer1">
      <timerEventDefinition><timeDuration>PT1H</timeDuration></timerEventDefinition>
      <outgoing>f5</outgoing>
    </intermediateThrowEvent>
    <sequenceFlow id="f5" sourceRef="timer1" targetRef="end1"/>
    <endEvent id="end1"/>
  </process>
</definitions>"#;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(xml)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "term-timer" }))
        .await
        .json();
    let inst_id = inst["id"].as_str().unwrap();

    // Claim and complete task_a → triggers terminate
    let tasks: Value = server.get("/v1/tasks").await.json();
    let task_a = tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["process_instance_id"] == inst_id && t["element_id"] == "task_a")
        .unwrap();
    let task_a_id = task_a["id"].as_str().unwrap();
    server
        .post(&format!("/v1/tasks/{task_a_id}/claim"))
        .json(&json!({"claimed_by": "test"}))
        .await
        .assert_status(StatusCode::OK);
    server
        .post(&format!("/v1/tasks/{task_a_id}/complete"))
        .json(&json!({}))
        .await
        .assert_status(StatusCode::OK);

    // Instance should be COMPLETED
    let inst_after: Value = server
        .get(&format!("/v1/process-instances/{inst_id}"))
        .await
        .json();
    assert_eq!(
        inst_after["state"], "COMPLETED",
        "Instance should be COMPLETED after terminate"
    );

    // Timer should be cancelled (no unfired timers remain)
    let timers: Value = server
        .get(&format!("/v1/process-instances/{inst_id}/timers"))
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
        0,
        "All timers should be cancelled after terminate"
    );
}

#[tokio::test]
async fn escalation_end_without_boundary_completes_normally() {
    let (server, _container) = setup().await;

    // Subprocess throws escalation but no boundary is defined → subprocess completes normally
    // and parent advances to task_after
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <escalation id="esc1" escalationCode="ESC_NONE"/>
  <process id="esc-no-boundary">
    <startEvent id="start"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start" targetRef="sub"/>
    <subProcess id="sub"><outgoing>f2</outgoing>
      <startEvent id="sub_s"><outgoing>sf1</outgoing></startEvent>
      <sequenceFlow id="sf1" sourceRef="sub_s" targetRef="sub_esc_end"/>
      <endEvent id="sub_esc_end">
        <escalationEventDefinition escalationRef="esc1"/>
      </endEvent>
    </subProcess>
    <sequenceFlow id="f2" sourceRef="sub" targetRef="task_after"/>
    <serviceTask id="task_after"><outgoing>f3</outgoing></serviceTask>
    <sequenceFlow id="f3" sourceRef="task_after" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;

    server
        .post("/v1/process-definitions")
        .content_type("text/xml")
        .text(xml)
        .await
        .assert_status(StatusCode::CREATED);

    let inst: Value = server
        .post("/v1/process-instances")
        .json(&json!({ "process_definition_id": "esc-no-boundary" }))
        .await
        .json();
    let inst_id = inst["id"].as_str().unwrap();

    // No boundary → subprocess completes normally → parent advances to task_after
    assert_eq!(inst["state"], "WAITING_FOR_TASK");

    let tasks: Value = server.get("/v1/tasks").await.json();
    let inst_tasks: Vec<&Value> = tasks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["process_instance_id"] == inst_id)
        .collect();
    assert_eq!(inst_tasks.len(), 1);
    assert_eq!(inst_tasks[0]["element_id"], "task_after");
}

// ── Event Subprocess Tests ────────────────────────────────────────────────────

const ESP_MESSAGE_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <message id="Msg1" name="cancel-order"/>
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1" name="Main Task"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="esp1" triggeredByEvent="true">
      <startEvent id="esp_start" isInterrupting="true">
        <messageEventDefinition messageRef="Msg1"/>
        <outgoing>sf_e1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_e1" sourceRef="esp_start" targetRef="esp_task"/>
      <serviceTask id="esp_task" name="Handle Cancel"><outgoing>sf_e2</outgoing></serviceTask>
      <sequenceFlow id="sf_e2" sourceRef="esp_task" targetRef="esp_end"/>
      <endEvent id="esp_end"/>
    </subProcess>
  </process>
</definitions>"#;

#[tokio::test]
async fn test_interrupting_message_esp_fires_on_message() {
    let (server, _container) = setup().await;

    let deploy = server
        .post("/v1/process-definitions")
        .content_type("application/xml")
        .bytes(ESP_MESSAGE_BPMN.into())
        .await;
    assert_eq!(deploy.status_code(), StatusCode::CREATED);
    let def_id = deploy.json::<Value>()["id"].as_str().unwrap().to_string();

    let start = server
        .post("/v1/process-instances")
        .json(&json!({"process_definition_id": def_id}))
        .await;
    assert_eq!(start.status_code(), StatusCode::CREATED);
    let instance_id = start.json::<Value>()["id"].as_str().unwrap().to_string();

    // Verify task1 is active
    let inst = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await;
    assert_eq!(inst.json::<Value>()["state"], "WAITING_FOR_TASK");

    // Send cancel-order message → triggers interrupting ESP
    let msg = server
        .post("/v1/messages")
        .json(&json!({"message_name": "cancel-order", "process_instance_id": instance_id}))
        .await;
    assert_eq!(msg.status_code(), StatusCode::OK);

    // Verify esp_task is now active (task1 was cancelled)
    let tasks: Value = server.get("/v1/tasks?state=CREATED").await.json();
    let task_names: Vec<_> = tasks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["process_instance_id"] == instance_id)
        .map(|t| t["element_id"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        task_names.contains(&"esp_task".to_string()),
        "esp_task should be active after message ESP fired, tasks: {:?}",
        task_names
    );
    assert!(
        !task_names.contains(&"task1".to_string()),
        "task1 should be cancelled: {:?}",
        task_names
    );
}

const ESP_SIGNAL_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <signal id="Sig1" name="alert"/>
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1" name="Main Task"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="esp1" triggeredByEvent="true">
      <startEvent id="esp_start" isInterrupting="false">
        <signalEventDefinition signalRef="Sig1"/>
        <outgoing>sf_e1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_e1" sourceRef="esp_start" targetRef="esp_task"/>
      <serviceTask id="esp_task" name="Handle Alert"><outgoing>sf_e2</outgoing></serviceTask>
      <sequenceFlow id="sf_e2" sourceRef="esp_task" targetRef="esp_end"/>
      <endEvent id="esp_end"/>
    </subProcess>
  </process>
</definitions>"#;

#[tokio::test]
async fn test_non_interrupting_signal_esp_runs_in_parallel() {
    let (server, _container) = setup().await;

    let deploy = server
        .post("/v1/process-definitions")
        .content_type("application/xml")
        .bytes(ESP_SIGNAL_BPMN.into())
        .await;
    assert_eq!(deploy.status_code(), StatusCode::CREATED);
    let def_id = deploy.json::<Value>()["id"].as_str().unwrap().to_string();

    let start = server
        .post("/v1/process-instances")
        .json(&json!({"process_definition_id": def_id}))
        .await;
    assert_eq!(start.status_code(), StatusCode::CREATED);
    let instance_id = start.json::<Value>()["id"].as_str().unwrap().to_string();

    // Send signal → triggers non-interrupting ESP
    let sig = server.post("/v1/signals/alert").json(&json!({})).await;
    assert_eq!(sig.status_code(), StatusCode::OK);

    // Verify both task1 AND esp_task are active
    let tasks: Value = server.get("/v1/tasks?state=CREATED").await.json();
    let task_names: Vec<_> = tasks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["process_instance_id"] == instance_id)
        .map(|t| t["element_id"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        task_names.contains(&"task1".to_string()),
        "task1 should still be active: {:?}",
        task_names
    );
    assert!(
        task_names.contains(&"esp_task".to_string()),
        "esp_task should be active in parallel: {:?}",
        task_names
    );
}

const ESP_TIMER_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1" name="Main Task"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="esp1" triggeredByEvent="true">
      <startEvent id="esp_start" isInterrupting="true">
        <timerEventDefinition><timeDuration>PT0S</timeDuration></timerEventDefinition>
        <outgoing>sf_e1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_e1" sourceRef="esp_start" targetRef="esp_task"/>
      <serviceTask id="esp_task" name="Timer Handler"><outgoing>sf_e2</outgoing></serviceTask>
      <sequenceFlow id="sf_e2" sourceRef="esp_task" targetRef="esp_end"/>
      <endEvent id="esp_end"/>
    </subProcess>
  </process>
</definitions>"#;

#[tokio::test]
async fn test_timer_esp_fires_after_due_at() {
    let (server, _container) = setup_with_scheduler().await;

    let deploy = server
        .post("/v1/process-definitions")
        .content_type("application/xml")
        .bytes(ESP_TIMER_BPMN.into())
        .await;
    assert_eq!(deploy.status_code(), StatusCode::CREATED);
    let def_id = deploy.json::<Value>()["id"].as_str().unwrap().to_string();

    let start = server
        .post("/v1/process-instances")
        .json(&json!({"process_definition_id": def_id}))
        .await;
    assert_eq!(start.status_code(), StatusCode::CREATED);
    let instance_id = start.json::<Value>()["id"].as_str().unwrap().to_string();

    // Wait for scheduler to fire the timer ESP (PT0S = immediate)
    tokio::time::sleep(tokio::time::Duration::from_secs(4)).await;

    // Verify esp_task is now active (interrupting: task1 cancelled)
    let tasks: Value = server.get("/v1/tasks?state=CREATED").await.json();
    let task_names: Vec<_> = tasks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["process_instance_id"] == instance_id)
        .map(|t| t["element_id"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        task_names.contains(&"esp_task".to_string()),
        "esp_task should be active after timer ESP fired, tasks: {:?}",
        task_names
    );
    assert!(
        !task_names.contains(&"task1".to_string()),
        "task1 should be cancelled: {:?}",
        task_names
    );
}

// ── Task 13 integration tests ──────────────────────────────────────────────

/// BPMN with a catch-all interrupting error ESP.
/// Fail task1 (no retries) → error ESP fires → esp_task becomes active.
const ESP_ERROR_CATCHALL_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1" name="Main Task"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="err_esp" triggeredByEvent="true">
      <startEvent id="err_esp_start" isInterrupting="true">
        <errorEventDefinition/>
        <outgoing>sf_e1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_e1" sourceRef="err_esp_start" targetRef="esp_task"/>
      <serviceTask id="esp_task" name="Handle Error"><outgoing>sf_e2</outgoing></serviceTask>
      <sequenceFlow id="sf_e2" sourceRef="esp_task" targetRef="esp_end"/>
      <endEvent id="esp_end"/>
    </subProcess>
  </process>
</definitions>"#;

#[tokio::test]
async fn test_catch_all_error_esp_fires_on_task_failure() {
    let (server, _c) = setup().await;

    let deploy = server
        .post("/v1/process-definitions")
        .content_type("application/xml")
        .bytes(ESP_ERROR_CATCHALL_BPMN.into())
        .await;
    assert_eq!(deploy.status_code(), StatusCode::CREATED);
    let def_id = deploy.json::<Value>()["id"].as_str().unwrap().to_string();

    let start = server
        .post("/v1/process-instances")
        .json(&json!({"process_definition_id": def_id}))
        .await;
    assert_eq!(start.status_code(), StatusCode::CREATED);
    let instance_id = start.json::<Value>()["id"].as_str().unwrap().to_string();

    // Find and claim task1
    let tasks: Value = server.get("/v1/tasks?state=CREATED").await.json();
    let task1 = tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["process_instance_id"] == instance_id && t["element_id"] == "task1")
        .expect("task1 should be active");
    let task_id = task1["id"].as_str().unwrap();

    // Claim task1 (fail requires CLAIMED state)
    server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({"claimed_by": "w1"}))
        .await
        .assert_status(StatusCode::OK);

    // Fail task1 → catch-all error ESP should fire
    server
        .post(&format!("/v1/tasks/{task_id}/fail"))
        .json(&json!({"reason": "something went wrong"}))
        .await
        .assert_status(StatusCode::OK);

    // Verify esp_task is now active and task1 is gone
    let tasks: Value = server.get("/v1/tasks?state=CREATED").await.json();
    let task_names: Vec<_> = tasks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["process_instance_id"] == instance_id)
        .map(|t| t["element_id"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        task_names.contains(&"esp_task".to_string()),
        "esp_task should be active after error ESP fired: {:?}",
        task_names
    );
    assert!(
        !task_names.contains(&"task1".to_string()),
        "task1 should be cancelled by interrupting error ESP: {:?}",
        task_names
    );

    // Complete esp_task → instance should complete
    let esp_tasks: Value = server.get("/v1/tasks?state=CREATED").await.json();
    let esp_task = esp_tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["process_instance_id"] == instance_id && t["element_id"] == "esp_task")
        .expect("esp_task should be found");
    let esp_task_id = esp_task["id"].as_str().unwrap();
    server
        .post(&format!("/v1/tasks/{esp_task_id}/claim"))
        .json(&json!({"claimed_by": "w1"}))
        .await
        .assert_status(StatusCode::OK);
    server
        .post(&format!("/v1/tasks/{esp_task_id}/complete"))
        .json(&json!({"variables": {}}))
        .await
        .assert_status(StatusCode::OK);

    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(
        inst["state"], "COMPLETED",
        "instance should be COMPLETED after ESP task done"
    );
}

/// BPMN with a non-interrupting message ESP.
/// After message arrives: both task1 AND esp_task must be active.
const ESP_MSG_NON_INTERRUPTING_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <message id="msg_ni" name="alert"/>
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1" name="Main Task"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="msg_esp" triggeredByEvent="true">
      <startEvent id="esp_start" isInterrupting="false">
        <messageEventDefinition messageRef="msg_ni"/>
        <outgoing>sf_e1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_e1" sourceRef="esp_start" targetRef="esp_task"/>
      <serviceTask id="esp_task" name="Handle Alert"><outgoing>sf_e2</outgoing></serviceTask>
      <sequenceFlow id="sf_e2" sourceRef="esp_task" targetRef="esp_end"/>
      <endEvent id="esp_end"/>
    </subProcess>
  </process>
</definitions>"#;

#[tokio::test]
async fn test_non_interrupting_message_esp_keeps_parent_task_alive() {
    let (server, _c) = setup().await;

    let deploy = server
        .post("/v1/process-definitions")
        .content_type("application/xml")
        .bytes(ESP_MSG_NON_INTERRUPTING_BPMN.into())
        .await;
    assert_eq!(deploy.status_code(), StatusCode::CREATED);
    let def_id = deploy.json::<Value>()["id"].as_str().unwrap().to_string();

    let start = server
        .post("/v1/process-instances")
        .json(&json!({"process_definition_id": def_id}))
        .await;
    assert_eq!(start.status_code(), StatusCode::CREATED);
    let instance_id = start.json::<Value>()["id"].as_str().unwrap().to_string();

    // Send message to trigger non-interrupting ESP
    let msg = server
        .post("/v1/messages")
        .json(&json!({"message_name": "alert", "process_instance_id": instance_id}))
        .await;
    assert_eq!(
        msg.status_code(),
        StatusCode::OK,
        "message should match ESP subscription"
    );

    // Both task1 AND esp_task must be active
    let tasks: Value = server.get("/v1/tasks?state=CREATED").await.json();
    let task_names: Vec<_> = tasks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["process_instance_id"] == instance_id)
        .map(|t| t["element_id"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        task_names.contains(&"task1".to_string()),
        "task1 should still be alive (non-interrupting): {:?}",
        task_names
    );
    assert!(
        task_names.contains(&"esp_task".to_string()),
        "esp_task should also be active: {:?}",
        task_names
    );
}

#[tokio::test]
async fn test_esp_subscription_deleted_when_scope_completes() {
    let (server, _c) = setup().await;

    let deploy = server
        .post("/v1/process-definitions")
        .content_type("application/xml")
        .bytes(ESP_MSG_NON_INTERRUPTING_BPMN.into())
        .await;
    assert_eq!(deploy.status_code(), StatusCode::CREATED);
    let def_id = deploy.json::<Value>()["id"].as_str().unwrap().to_string();

    let start = server
        .post("/v1/process-instances")
        .json(&json!({"process_definition_id": def_id}))
        .await;
    assert_eq!(start.status_code(), StatusCode::CREATED);
    let instance_id = start.json::<Value>()["id"].as_str().unwrap().to_string();

    // Complete task1 without triggering ESP
    let tasks: Value = server.get("/v1/tasks?state=CREATED").await.json();
    let task = tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["process_instance_id"] == instance_id && t["element_id"] == "task1")
        .expect("task1 should be active");
    let task_id = task["id"].as_str().unwrap();
    server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({"claimed_by": "w1"}))
        .await
        .assert_status(StatusCode::OK);
    server
        .post(&format!("/v1/tasks/{task_id}/complete"))
        .json(&json!({"variables": {}}))
        .await
        .assert_status(StatusCode::OK);

    // Instance should be COMPLETED
    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "COMPLETED", "instance should be COMPLETED");

    // Sending message now should NOT find a match (subscription was cleaned up)
    let msg = server
        .post("/v1/messages")
        .json(&json!({"message_name": "alert", "process_instance_id": instance_id}))
        .await;
    // Completed instance has no active subscriptions — expect 404 or not-found response
    assert!(
        msg.status_code() == StatusCode::NOT_FOUND
            || msg.status_code() == StatusCode::UNPROCESSABLE_ENTITY,
        "message should not match completed instance, got: {}",
        msg.status_code()
    );
}

/// BPMN with a non-interrupting timer ESP (PT0S).
/// Scheduler fires timer → esp_task created alongside task1.
const ESP_TIMER_NON_INTERRUPTING_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1" name="Main Task"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="ni_timer_esp" triggeredByEvent="true">
      <startEvent id="ni_esp_start" isInterrupting="false">
        <timerEventDefinition><timeDuration>PT0S</timeDuration></timerEventDefinition>
        <outgoing>sf_e1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_e1" sourceRef="ni_esp_start" targetRef="ni_esp_task"/>
      <serviceTask id="ni_esp_task" name="Timer Side Effect"><outgoing>sf_e2</outgoing></serviceTask>
      <sequenceFlow id="sf_e2" sourceRef="ni_esp_task" targetRef="ni_esp_end"/>
      <endEvent id="ni_esp_end"/>
    </subProcess>
  </process>
</definitions>"#;

#[tokio::test]
async fn test_timer_esp_non_interrupting_fires_alongside_parent() {
    let (server, _c) = setup_with_scheduler().await;

    let deploy = server
        .post("/v1/process-definitions")
        .content_type("application/xml")
        .bytes(ESP_TIMER_NON_INTERRUPTING_BPMN.into())
        .await;
    assert_eq!(deploy.status_code(), StatusCode::CREATED);
    let def_id = deploy.json::<Value>()["id"].as_str().unwrap().to_string();

    let start = server
        .post("/v1/process-instances")
        .json(&json!({"process_definition_id": def_id}))
        .await;
    assert_eq!(start.status_code(), StatusCode::CREATED);
    let instance_id = start.json::<Value>()["id"].as_str().unwrap().to_string();

    // Wait for scheduler (2s tick, 4s total)
    tokio::time::sleep(tokio::time::Duration::from_secs(4)).await;

    // Both task1 AND ni_esp_task should be active (non-interrupting)
    let tasks: Value = server.get("/v1/tasks?state=CREATED").await.json();
    let task_names: Vec<_> = tasks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["process_instance_id"] == instance_id)
        .map(|t| t["element_id"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        task_names.contains(&"task1".to_string()),
        "task1 should still be alive (non-interrupting): {:?}",
        task_names
    );
    assert!(
        task_names.contains(&"ni_esp_task".to_string()),
        "ni_esp_task should be active alongside task1: {:?}",
        task_names
    );
}

// ── Additional gap-filling tests ───────────────────────────────────────────

/// Signal ESP interrupting: signal cancels parent task, esp_task takes over.
const ESP_SIGNAL_INTERRUPTING_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <signal id="sig_stop" name="stop-work"/>
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1" name="Main Task"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="sig_esp" triggeredByEvent="true">
      <startEvent id="sig_esp_start" isInterrupting="true">
        <signalEventDefinition signalRef="sig_stop"/>
        <outgoing>sf_e1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_e1" sourceRef="sig_esp_start" targetRef="sig_esp_task"/>
      <serviceTask id="sig_esp_task" name="Handle Stop"><outgoing>sf_e2</outgoing></serviceTask>
      <sequenceFlow id="sf_e2" sourceRef="sig_esp_task" targetRef="sig_esp_end"/>
      <endEvent id="sig_esp_end"/>
    </subProcess>
  </process>
</definitions>"#;

#[tokio::test]
async fn test_interrupting_signal_esp_cancels_parent_task() {
    let (server, _c) = setup().await;

    let deploy = server
        .post("/v1/process-definitions")
        .content_type("application/xml")
        .bytes(ESP_SIGNAL_INTERRUPTING_BPMN.into())
        .await;
    assert_eq!(deploy.status_code(), StatusCode::CREATED);
    let def_id = deploy.json::<Value>()["id"].as_str().unwrap().to_string();

    let start = server
        .post("/v1/process-instances")
        .json(&json!({"process_definition_id": def_id}))
        .await;
    assert_eq!(start.status_code(), StatusCode::CREATED);
    let instance_id = start.json::<Value>()["id"].as_str().unwrap().to_string();

    // Broadcast signal → interrupting ESP fires, cancels task1
    let sig = server.post("/v1/signals/stop-work").json(&json!({})).await;
    assert_eq!(sig.status_code(), StatusCode::OK);

    let tasks: Value = server.get("/v1/tasks?state=CREATED").await.json();
    let task_names: Vec<_> = tasks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["process_instance_id"] == instance_id)
        .map(|t| t["element_id"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        task_names.contains(&"sig_esp_task".to_string()),
        "sig_esp_task should be active: {:?}",
        task_names
    );
    assert!(
        !task_names.contains(&"task1".to_string()),
        "task1 should be cancelled by interrupting signal ESP: {:?}",
        task_names
    );
}

/// Interrupting escalation ESP: process completes task1 → escalation throw fires inline
/// → esp_task becomes active.
const ESP_ESCALATION_INTERRUPTING_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1" name="Main Task"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="esc_throw"/>
    <intermediateThrowEvent id="esc_throw" name="Escalate">
      <escalationEventDefinition escalationCode="NEED_REVIEW"/>
      <outgoing>sf3</outgoing>
    </intermediateThrowEvent>
    <sequenceFlow id="sf3" sourceRef="esc_throw" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="esc_esp" triggeredByEvent="true">
      <startEvent id="esc_esp_start" isInterrupting="true">
        <escalationEventDefinition escalationCode="NEED_REVIEW"/>
        <outgoing>sf_e1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_e1" sourceRef="esc_esp_start" targetRef="esp_task"/>
      <serviceTask id="esp_task" name="Review Handler"><outgoing>sf_e2</outgoing></serviceTask>
      <sequenceFlow id="sf_e2" sourceRef="esp_task" targetRef="esp_end"/>
      <endEvent id="esp_end"/>
    </subProcess>
  </process>
</definitions>"#;

#[tokio::test]
async fn test_interrupting_escalation_esp_fires_on_escalation_throw() {
    let (server, _c) = setup().await;

    let deploy = server
        .post("/v1/process-definitions")
        .content_type("application/xml")
        .bytes(ESP_ESCALATION_INTERRUPTING_BPMN.into())
        .await;
    assert_eq!(deploy.status_code(), StatusCode::CREATED);
    let def_id = deploy.json::<Value>()["id"].as_str().unwrap().to_string();

    let start = server
        .post("/v1/process-instances")
        .json(&json!({"process_definition_id": def_id}))
        .await;
    assert_eq!(start.status_code(), StatusCode::CREATED);
    let instance_id = start.json::<Value>()["id"].as_str().unwrap().to_string();

    // Complete task1 → engine advances to escalation throw → ESP fires inline
    let tasks: Value = server.get("/v1/tasks?state=CREATED").await.json();
    let task = tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["process_instance_id"] == instance_id && t["element_id"] == "task1")
        .expect("task1 should be active");
    let task_id = task["id"].as_str().unwrap();
    server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({"claimed_by": "w1"}))
        .await
        .assert_status(StatusCode::OK);
    server
        .post(&format!("/v1/tasks/{task_id}/complete"))
        .json(&json!({"variables": {}}))
        .await
        .assert_status(StatusCode::OK);

    // esp_task should now be active (escalation ESP fired)
    let tasks: Value = server.get("/v1/tasks?state=CREATED").await.json();
    let task_names: Vec<_> = tasks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["process_instance_id"] == instance_id)
        .map(|t| t["element_id"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        task_names.contains(&"esp_task".to_string()),
        "esp_task should be active after escalation ESP fired: {:?}",
        task_names
    );

    // Complete esp_task → instance should complete
    let esp_task = tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["process_instance_id"] == instance_id && t["element_id"] == "esp_task")
        .expect("esp_task should be in tasks list");
    let esp_task_id = esp_task["id"].as_str().unwrap();
    server
        .post(&format!("/v1/tasks/{esp_task_id}/claim"))
        .json(&json!({"claimed_by": "w1"}))
        .await
        .assert_status(StatusCode::OK);
    server
        .post(&format!("/v1/tasks/{esp_task_id}/complete"))
        .json(&json!({"variables": {}}))
        .await
        .assert_status(StatusCode::OK);

    let inst: Value = server
        .get(&format!("/v1/process-instances/{instance_id}"))
        .await
        .json();
    assert_eq!(inst["state"], "COMPLETED");
}

/// Non-interrupting escalation ESP: task1 completes → escalation throw fires inline
/// → both esp_task AND subsequent main flow run in parallel.
const ESP_ESCALATION_NON_INTERRUPTING_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1" name="Main Task"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="esc_throw"/>
    <intermediateThrowEvent id="esc_throw" name="Escalate">
      <escalationEventDefinition escalationCode="NOTIFY"/>
      <outgoing>sf3</outgoing>
    </intermediateThrowEvent>
    <sequenceFlow id="sf3" sourceRef="esc_throw" targetRef="task2"/>
    <serviceTask id="task2" name="Continue Work"><outgoing>sf4</outgoing></serviceTask>
    <sequenceFlow id="sf4" sourceRef="task2" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="ni_esc_esp" triggeredByEvent="true">
      <startEvent id="ni_esc_start" isInterrupting="false">
        <escalationEventDefinition escalationCode="NOTIFY"/>
        <outgoing>sf_e1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_e1" sourceRef="ni_esc_start" targetRef="ni_esp_task"/>
      <serviceTask id="ni_esp_task" name="Notify Handler"><outgoing>sf_e2</outgoing></serviceTask>
      <sequenceFlow id="sf_e2" sourceRef="ni_esp_task" targetRef="ni_esp_end"/>
      <endEvent id="ni_esp_end"/>
    </subProcess>
  </process>
</definitions>"#;

#[tokio::test]
async fn test_non_interrupting_escalation_esp_runs_alongside_main_flow() {
    let (server, _c) = setup().await;

    let deploy = server
        .post("/v1/process-definitions")
        .content_type("application/xml")
        .bytes(ESP_ESCALATION_NON_INTERRUPTING_BPMN.into())
        .await;
    assert_eq!(deploy.status_code(), StatusCode::CREATED);
    let def_id = deploy.json::<Value>()["id"].as_str().unwrap().to_string();

    let start = server
        .post("/v1/process-instances")
        .json(&json!({"process_definition_id": def_id}))
        .await;
    assert_eq!(start.status_code(), StatusCode::CREATED);
    let instance_id = start.json::<Value>()["id"].as_str().unwrap().to_string();

    // Complete task1 → escalation throw fires → both task2 and ni_esp_task become active
    let tasks: Value = server.get("/v1/tasks?state=CREATED").await.json();
    let task = tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["process_instance_id"] == instance_id && t["element_id"] == "task1")
        .expect("task1 should be active");
    let task_id = task["id"].as_str().unwrap();
    server
        .post(&format!("/v1/tasks/{task_id}/claim"))
        .json(&json!({"claimed_by": "w1"}))
        .await
        .assert_status(StatusCode::OK);
    server
        .post(&format!("/v1/tasks/{task_id}/complete"))
        .json(&json!({"variables": {}}))
        .await
        .assert_status(StatusCode::OK);

    // Both task2 (main flow continuation) AND ni_esp_task (ESP) should be active
    let tasks: Value = server.get("/v1/tasks?state=CREATED").await.json();
    let task_names: Vec<_> = tasks
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["process_instance_id"] == instance_id)
        .map(|t| t["element_id"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        task_names.contains(&"task2".to_string()),
        "task2 (main continuation) should be active: {:?}",
        task_names
    );
    assert!(
        task_names.contains(&"ni_esp_task".to_string()),
        "ni_esp_task should be active alongside task2: {:?}",
        task_names
    );
}
