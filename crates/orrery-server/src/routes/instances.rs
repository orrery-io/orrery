use crate::errors::{codes, ApiError, ErrorResponse};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use std::collections::HashMap;
use utoipa::ToSchema;
use uuid::Uuid;

use orrery::engine::{ActiveElement, Engine, WaitState};
use orrery::model::{
    EventSubProcessSubscription, EventSubProcessTrigger, FlowElement, VisitEvent, VisitedElement,
};
use orrery::parser::parse_bpmn;

/// Derive the DB state string from per-element active_elements.
pub fn derive_db_state(
    active_elements: &[ActiveElement],
    is_completed: bool,
    is_failed: bool,
) -> &'static str {
    if is_completed {
        return "COMPLETED";
    }
    if is_failed {
        return "FAILED";
    }
    if active_elements.is_empty() {
        return "RUNNING";
    }
    let first = &active_elements[0].wait_state;
    let all_same = active_elements
        .iter()
        .all(|e| std::mem::discriminant(&e.wait_state) == std::mem::discriminant(first));
    if !all_same {
        return "RUNNING";
    }
    match first {
        WaitState::Task { .. } => "WAITING_FOR_TASK",
        WaitState::Timer { .. } => "WAITING_FOR_TIMER",
        WaitState::Message { .. } => "WAITING_FOR_MESSAGE",
        WaitState::Signal { .. } => "WAITING_FOR_SIGNAL",
    }
}

/// Persist engine internals (join_counts, loop_state) into process variables.
/// Must be called after every engine advance and before persisting variables to DB.
pub fn save_engine_internals(
    engine: &orrery::engine::Engine,
    variables: &mut HashMap<String, serde_json::Value>,
) {
    if !engine.join_counts().is_empty() {
        variables.insert(
            "__join_counts__".to_string(),
            serde_json::to_value(engine.join_counts()).unwrap(),
        );
    } else {
        variables.remove("__join_counts__");
    }
    if !engine.loop_state().is_empty() {
        variables.insert(
            "__loop_state__".to_string(),
            serde_json::to_value(engine.loop_state()).unwrap(),
        );
    } else {
        variables.remove("__loop_state__");
    }
    if !engine.inclusive_join_counts().is_empty() {
        variables.insert(
            "__inclusive_join_counts__".to_string(),
            serde_json::to_value(engine.inclusive_join_counts()).unwrap(),
        );
    } else {
        variables.remove("__inclusive_join_counts__");
    }
}

/// If any of the new elements were activated by an EventBasedGateway, return its ID.
/// Detection: check if any of the incoming flows of the new elements originates from an EBG.
fn find_event_gateway_group(
    new_elements: &[&orrery::engine::ActiveElement],
    definition: &orrery::model::ProcessDefinition,
) -> Option<String> {
    if new_elements.len() < 2 {
        return None;
    }

    for new_elem in new_elements {
        for flow in &definition.sequence_flows {
            if flow.target_ref == new_elem.element_id {
                if let Some(FlowElement::EventBasedGateway(_)) = definition
                    .elements
                    .iter()
                    .find(|e| e.id() == flow.source_ref.as_str())
                {
                    return Some(flow.source_ref.clone());
                }
            }
        }
    }
    None
}

/// Cancel sibling subscriptions in an event-based gateway group.
/// Deletes all subscriptions sharing the same group_id except those belonging to the winning element.
pub async fn cancel_sibling_subscriptions(
    conn: &mut sqlx::PgConnection,
    winning_element_id: &str,
    group_id: &str,
) -> Result<(), ApiError> {
    sqlx::query!(
        "DELETE FROM message_subscriptions WHERE event_gateway_group_id = $1 AND element_id != $2",
        group_id,
        winning_element_id,
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    sqlx::query!(
        "DELETE FROM scheduled_timers WHERE event_gateway_group_id = $1 AND element_id != $2",
        group_id,
        winning_element_id,
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    sqlx::query!(
        "DELETE FROM signal_subscriptions WHERE event_gateway_group_id = $1 AND element_id != $2",
        group_id,
        winning_element_id,
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(())
}

/// Cancel all remaining side effects for an instance that has terminated/completed.
/// Called when a TerminateEndEvent (or similar) fires, killing all parallel branches.
/// Cancels uncompleted tasks, unfired timers, and pending subscriptions.
pub async fn cancel_all_remaining_side_effects(
    conn: &mut sqlx::PgConnection,
    instance_id: &str,
    completed_task_id: Option<&str>,
) -> Result<(), ApiError> {
    // Cancel all non-completed tasks (except the one that triggered this)
    if let Some(task_id) = completed_task_id {
        sqlx::query(
            "UPDATE tasks SET state = 'CANCELLED', completed_at = NOW() \
             WHERE process_instance_id = $1 AND state IN ('CREATED', 'CLAIMED') AND id != $2",
        )
        .bind(instance_id)
        .bind(task_id)
        .execute(&mut *conn)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    } else {
        sqlx::query(
            "UPDATE tasks SET state = 'CANCELLED', completed_at = NOW() \
             WHERE process_instance_id = $1 AND state IN ('CREATED', 'CLAIMED')",
        )
        .bind(instance_id)
        .execute(&mut *conn)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    }
    // Cancel unfired timers
    sqlx::query(
        "UPDATE scheduled_timers SET fired = TRUE, fired_at = NOW() \
         WHERE process_instance_id = $1 AND fired = FALSE",
    )
    .bind(instance_id)
    .execute(&mut *conn)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    // Delete pending message subscriptions
    sqlx::query("DELETE FROM message_subscriptions WHERE process_instance_id = $1")
        .bind(instance_id)
        .execute(&mut *conn)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    // Delete pending signal subscriptions
    sqlx::query("DELETE FROM signal_subscriptions WHERE process_instance_id = $1")
        .bind(instance_id)
        .execute(&mut *conn)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(())
}

/// Create side effects (tasks, timers, subscriptions) for newly activated elements.
/// Only elements NOT in `previous_ids` will get side effects created.
///
/// Accepts `&mut PgConnection` so it works inside transactions (reborrow with `&mut *tx`)
/// and with pool-acquired connections (`&mut *pool.acquire().await?`).
pub async fn create_side_effects_for_new_elements(
    conn: &mut sqlx::PgConnection,
    instance_id: &str,
    result: &orrery::engine::ExecutionResult,
    previous_ids: &std::collections::HashSet<String>,
    definition: &orrery::model::ProcessDefinition,
    variables_json: &serde_json::Value,
    max_retries: i32,
) -> Result<(), ApiError> {
    // Detect event-based gateway group for new elements
    let new_elements: Vec<&ActiveElement> = result
        .active_elements
        .iter()
        .filter(|e| !previous_ids.contains(&e.element_id))
        .collect();

    let group_id_str;
    let event_group_id: Option<&str> =
        if find_event_gateway_group(&new_elements, definition).is_some() {
            group_id_str = uuid::Uuid::new_v4().to_string();
            Some(&group_id_str)
        } else {
            None
        };

    for elem in &result.active_elements {
        if previous_ids.contains(&elem.element_id) {
            continue; // already waiting — side effect already exists
        }
        match &elem.wait_state {
            WaitState::Task { topic } => {
                create_task_for_element(
                    &mut *conn,
                    instance_id,
                    &elem.element_id,
                    variables_json,
                    max_retries,
                    topic.as_deref(),
                )
                .await?;
                // Create message boundary subscriptions for this task
                for belem in &definition.elements {
                    if let FlowElement::MessageBoundaryEvent(mb) = belem {
                        if mb.attached_to_ref == elem.element_id {
                            let ck_value = mb.correlation_key.as_deref().and_then(|expr| {
                                orrery::expression::eval_to_string(expr, &result.variables)
                            });
                            create_message_boundary_subscription(
                                &mut *conn,
                                instance_id,
                                &mb.id,
                                &elem.element_id,
                                &mb.message_name,
                                ck_value.as_deref(),
                                mb.is_interrupting,
                            )
                            .await?;
                        }
                    }
                }
                // Create signal boundary subscriptions for this task
                for belem in &definition.elements {
                    if let FlowElement::SignalBoundaryEvent(sb) = belem {
                        if sb.attached_to_ref == elem.element_id {
                            create_signal_boundary_subscription(
                                &mut *conn,
                                instance_id,
                                &sb.id,
                                &elem.element_id,
                                &sb.signal_ref,
                                sb.is_interrupting,
                            )
                            .await?;
                        }
                    }
                }
                // Schedule timer boundary events attached to this task
                for belem in &definition.elements {
                    if let FlowElement::TimerBoundaryEvent(tb) = belem {
                        if tb.attached_to_ref == elem.element_id {
                            schedule_timer(&mut *conn, instance_id, &tb.id, &tb.timer, None)
                                .await?;
                        }
                    }
                }
            }
            WaitState::Timer {
                definition: timer_def,
            } => {
                schedule_timer(
                    &mut *conn,
                    instance_id,
                    &elem.element_id,
                    timer_def,
                    event_group_id,
                )
                .await?;
            }
            WaitState::Message {
                message_name,
                correlation_key_expr,
            } => {
                let ck_value = correlation_key_expr
                    .as_deref()
                    .and_then(|expr| orrery::expression::eval_to_string(expr, &result.variables));
                create_message_subscription(
                    &mut *conn,
                    instance_id,
                    &elem.element_id,
                    message_name,
                    ck_value.as_deref(),
                    event_group_id,
                )
                .await?;
                // Create message boundary subscriptions for ReceiveTask
                for belem in &definition.elements {
                    if let FlowElement::MessageBoundaryEvent(mb) = belem {
                        if mb.attached_to_ref == elem.element_id {
                            let boundary_ck = mb.correlation_key.as_deref().and_then(|expr| {
                                orrery::expression::eval_to_string(expr, &result.variables)
                            });
                            create_message_boundary_subscription(
                                &mut *conn,
                                instance_id,
                                &mb.id,
                                &elem.element_id,
                                &mb.message_name,
                                boundary_ck.as_deref(),
                                mb.is_interrupting,
                            )
                            .await?;
                        }
                    }
                }
                // Create signal boundary subscriptions for this receive task
                for belem in &definition.elements {
                    if let FlowElement::SignalBoundaryEvent(sb) = belem {
                        if sb.attached_to_ref == elem.element_id {
                            create_signal_boundary_subscription(
                                &mut *conn,
                                instance_id,
                                &sb.id,
                                &elem.element_id,
                                &sb.signal_ref,
                                sb.is_interrupting,
                            )
                            .await?;
                        }
                    }
                }
                // Schedule timer boundary events attached to this receive task
                for belem in &definition.elements {
                    if let FlowElement::TimerBoundaryEvent(tb) = belem {
                        if tb.attached_to_ref == elem.element_id {
                            schedule_timer(&mut *conn, instance_id, &tb.id, &tb.timer, None)
                                .await?;
                        }
                    }
                }
            }
            WaitState::Signal { signal_ref } => {
                create_signal_subscription(
                    &mut *conn,
                    instance_id,
                    &elem.element_id,
                    signal_ref,
                    event_group_id,
                )
                .await?;
            }
        }
    }
    Ok(())
}

/// Sync event subprocess subscriptions for an instance.
/// Deletes rows for ESPs no longer in the desired set, inserts rows for new ones.
/// Called after every state-changing operation.
pub async fn sync_event_subprocess_subscriptions(
    conn: &mut sqlx::PgConnection,
    instance_id: &str,
    subscriptions: &[EventSubProcessSubscription],
) -> Result<(), ApiError> {
    // Get current rows
    let existing: Vec<String> = sqlx::query_scalar(
        "SELECT esp_id FROM event_subprocess_subscriptions WHERE process_instance_id = $1",
    )
    .bind(instance_id)
    .fetch_all(&mut *conn)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let desired_ids: std::collections::HashSet<&str> =
        subscriptions.iter().map(|s| s.esp_id.as_str()).collect();
    let existing_ids: std::collections::HashSet<String> = existing.into_iter().collect();

    // Delete rows no longer needed
    for old_id in existing_ids
        .iter()
        .filter(|id| !desired_ids.contains(id.as_str()))
    {
        sqlx::query("DELETE FROM event_subprocess_subscriptions WHERE process_instance_id = $1 AND esp_id = $2")
            .bind(instance_id).bind(old_id).execute(&mut *conn).await
            .map_err(|e| ApiError::internal(e.to_string()))?;
    }

    // Insert new rows
    for sub in subscriptions
        .iter()
        .filter(|s| !existing_ids.contains(&s.esp_id))
    {
        let id = uuid::Uuid::new_v4().to_string();
        match &sub.trigger {
            EventSubProcessTrigger::Message {
                message_name,
                correlation_key,
            } => {
                sqlx::query(
                    "INSERT INTO event_subprocess_subscriptions
                     (id, process_instance_id, esp_id, scope_id, trigger_type, message_name, correlation_key, is_interrupting)
                     VALUES ($1,$2,$3,$4,'message',$5,$6,$7)"
                ).bind(&id).bind(instance_id).bind(&sub.esp_id).bind(&sub.scope_id)
                 .bind(message_name).bind(correlation_key).bind(sub.is_interrupting)
                 .execute(&mut *conn).await
                 .map_err(|e| ApiError::internal(e.to_string()))?;
            }
            EventSubProcessTrigger::Signal { signal_ref } => {
                sqlx::query(
                    "INSERT INTO event_subprocess_subscriptions
                     (id, process_instance_id, esp_id, scope_id, trigger_type, signal_ref, is_interrupting)
                     VALUES ($1,$2,$3,$4,'signal',$5,$6)"
                ).bind(&id).bind(instance_id).bind(&sub.esp_id).bind(&sub.scope_id)
                 .bind(signal_ref).bind(sub.is_interrupting)
                 .execute(&mut *conn).await
                 .map_err(|e| ApiError::internal(e.to_string()))?;
            }
            EventSubProcessTrigger::Timer { timer } => {
                let due_at = crate::timer_eval::evaluate_due_at(timer).ok();
                sqlx::query(
                    "INSERT INTO event_subprocess_subscriptions
                     (id, process_instance_id, esp_id, scope_id, trigger_type,
                      timer_expression, timer_kind, due_at, is_interrupting)
                     VALUES ($1,$2,$3,$4,'timer',$5,$6,$7,$8)",
                )
                .bind(&id)
                .bind(instance_id)
                .bind(&sub.esp_id)
                .bind(&sub.scope_id)
                .bind(&timer.expression)
                .bind(format!("{:?}", timer.kind))
                .bind(due_at)
                .bind(sub.is_interrupting)
                .execute(&mut *conn)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;
            }
            _ => {} // Error/Escalation handled synchronously, not stored
        }
    }
    Ok(())
}

#[derive(Deserialize, ToSchema)]
pub struct StartInstanceRequest {
    /// ID of the process definition to instantiate
    pub process_definition_id: String,
    /// Optional business key — a stable external identifier for this instance (e.g. order ID).
    /// Used as a fallback correlation handle when sending messages.
    pub business_key: Option<String>,
    /// Initial process variables (arbitrary JSON object)
    #[serde(default)]
    pub variables: HashMap<String, Value>,
    /// Maximum number of times a task can be retried on failure (0 = no retries)
    #[serde(default)]
    pub max_retries: i32,
}

#[derive(Deserialize)]
pub struct InstanceListQuery {
    pub definition_id: Option<String>,
    pub state: Option<String>,
    pub version: Option<i32>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Serialize, ToSchema)]
pub struct ProcessInstanceResponse {
    pub id: String,
    pub process_definition_id: String,
    pub process_definition_version: i32,
    /// Stable external identifier for this instance (e.g. order ID), set at start time.
    pub business_key: Option<String>,
    /// One of: RUNNING, WAITING_FOR_TASK, COMPLETED, FAILED
    pub state: String,
    pub variables: Value,
    /// IDs of BPMN elements currently holding a token
    pub active_element_ids: Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    pub error_message: Option<String>,
    /// For FAILED instances: element_id of the most recent failed task.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_at_element_id: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct PaginatedInstancesResponse {
    pub items: Vec<ProcessInstanceResponse>,
    pub total: i64,
    pub page: u32,
    pub page_size: u32,
    pub total_pages: u32,
}

#[utoipa::path(
    get,
    path = "/v1/process-instances",
    params(
        ("definition_id" = Option<String>, Query, description = "Filter by process definition ID"),
        ("state" = Option<String>, Query, description = "Filter by state"),
        ("version" = Option<i32>, Query, description = "Filter by process definition version"),
        ("page" = Option<u32>, Query, description = "Page number (1-based, default 1)"),
        ("page_size" = Option<u32>, Query, description = "Items per page (default 20, max 100)"),
    ),
    responses(
        (status = 200, description = "Paginated list of process instances", body = PaginatedInstancesResponse),
        (status = 500, description = "Server error", body = ErrorResponse),
    ),
    tag = "Process Instances"
)]
pub async fn list(
    State(pool): State<PgPool>,
    Query(q): Query<InstanceListQuery>,
) -> Result<Json<PaginatedInstancesResponse>, ApiError> {
    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(20).clamp(1, 100);
    let offset = ((page - 1) * page_size) as i64;
    let limit = page_size as i64;

    let rows = sqlx::query!(
        r#"
        SELECT
            pi.id, pi.process_definition_id, pi.process_definition_version,
            pi.business_key, pi.state, pi.variables,
            pi.active_element_ids, pi.created_at, pi.ended_at, pi.error_message,
            (
                SELECT t.element_id
                FROM tasks t
                WHERE t.process_instance_id = pi.id AND t.state = 'FAILED'
                ORDER BY t.completed_at DESC NULLS LAST
                LIMIT 1
            ) AS failed_at_element_id,
            COUNT(*) OVER() AS "total_count!: i64"
        FROM process_instances pi
        WHERE ($1::text IS NULL OR pi.process_definition_id = $1)
          AND ($2::text IS NULL OR pi.state = $2)
          AND ($3::int IS NULL OR pi.process_definition_version = $3)
        ORDER BY pi.created_at DESC
        LIMIT $4 OFFSET $5
        "#,
        q.definition_id,
        q.state,
        q.version,
        limit,
        offset,
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let total = rows.first().map(|r| r.total_count).unwrap_or(0);
    let total_pages = if total == 0 {
        1
    } else {
        ((total as f64) / (page_size as f64)).ceil() as u32
    };

    let items = rows
        .into_iter()
        .map(|r| ProcessInstanceResponse {
            id: r.id,
            process_definition_id: r.process_definition_id,
            process_definition_version: r.process_definition_version,
            business_key: r.business_key,
            state: r.state,
            variables: r.variables,
            active_element_ids: r.active_element_ids,
            created_at: r.created_at,
            ended_at: r.ended_at,
            error_message: r.error_message,
            failed_at_element_id: r.failed_at_element_id,
        })
        .collect();

    Ok(Json(PaginatedInstancesResponse {
        items,
        total,
        page,
        page_size,
        total_pages,
    }))
}

#[utoipa::path(
    post,
    path = "/v1/process-instances",
    request_body(
        content = StartInstanceRequest,
        description = "Process definition ID and optional initial variables",
        content_type = "application/json"
    ),
    responses(
        (status = 201, description = "Instance started", body = ProcessInstanceResponse),
        (status = 404, description = "Process definition not found", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse),
    ),
    tag = "Process Instances"
)]
pub async fn start(
    State(pool): State<PgPool>,
    Json(req): Json<StartInstanceRequest>,
) -> Result<(StatusCode, Json<ProcessInstanceResponse>), ApiError> {
    // Parse optional version suffix: "invoice_process:3" → id="invoice_process", version=3
    // "invoice_process" → id="invoice_process", version=MAX
    let (def_id, def_version, bpmn_xml) = {
        let (id_part, ver_opt) = match req.process_definition_id.rsplit_once(':') {
            Some((id, ver_str)) => match ver_str.parse::<i32>() {
                Ok(v) => (id.to_string(), Some(v)),
                Err(_) => (req.process_definition_id.clone(), None),
            },
            None => (req.process_definition_id.clone(), None),
        };

        match ver_opt {
            Some(ver) => {
                let row = sqlx::query!(
                    "SELECT id, version, bpmn_xml FROM process_definitions WHERE id = $1 AND version = $2",
                    id_part,
                    ver,
                )
                .fetch_optional(&pool)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .ok_or_else(|| ApiError::not_found(codes::DEFINITION_NOT_FOUND, format!("Process definition '{}:{}' not found", id_part, ver)))?;
                (row.id, row.version, row.bpmn_xml)
            }
            None => {
                let row = sqlx::query!(
                    r#"
                    SELECT id, version, bpmn_xml
                    FROM process_definitions
                    WHERE id = $1
                    ORDER BY version DESC
                    LIMIT 1
                    "#,
                    id_part,
                )
                .fetch_optional(&pool)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .ok_or_else(|| {
                    ApiError::not_found(
                        codes::DEFINITION_NOT_FOUND,
                        format!("Process definition '{}' not found", id_part),
                    )
                })?;
                (row.id, row.version, row.bpmn_xml)
            }
        }
    };

    let definition = parse_bpmn(&bpmn_xml).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Parse error: {e}"),
        )
    })?;

    let mut engine = Engine::new(definition);
    let result = engine
        .start(req.variables)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let instance_id = Uuid::new_v4().to_string();
    let previous_ids = std::collections::HashSet::new(); // empty — all elements are new on start
    let active_ids: Vec<String> = result
        .active_elements
        .iter()
        .map(|e| e.element_id.clone())
        .collect();
    let state_str = derive_db_state(
        &result.active_elements,
        result.is_completed,
        result.is_failed,
    );
    let mut vars_to_save = result.variables.clone();
    save_engine_internals(&engine, &mut vars_to_save);
    let variables_json = serde_json::to_value(&vars_to_save).unwrap();
    let active_ids_json = serde_json::to_value(&active_ids).unwrap();
    let ended_at: Option<chrono::DateTime<chrono::Utc>> =
        result.is_completed.then(chrono::Utc::now);

    let db_row = sqlx::query!(
        r#"
        INSERT INTO process_instances (id, process_definition_id, process_definition_version, business_key, state, variables, active_element_ids, ended_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id, process_definition_id, process_definition_version, business_key, state, variables, active_element_ids, created_at, ended_at
        "#,
        instance_id,
        def_id,
        def_version,
        req.business_key,
        state_str,
        variables_json,
        active_ids_json,
        ended_at,
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    persist_visited_history(&mut conn, &instance_id, &result.visited, &variables_json).await?;
    create_side_effects_for_new_elements(
        &mut conn,
        &instance_id,
        &result,
        &previous_ids,
        engine.definition(),
        &variables_json,
        req.max_retries,
    )
    .await?;
    sync_event_subprocess_subscriptions(
        &mut conn,
        &instance_id,
        &result.event_subprocess_subscriptions,
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(ProcessInstanceResponse {
            id: db_row.id,
            process_definition_id: db_row.process_definition_id,
            process_definition_version: db_row.process_definition_version,
            business_key: db_row.business_key,
            state: db_row.state,
            variables: db_row.variables,
            active_element_ids: db_row.active_element_ids,
            created_at: db_row.created_at,
            ended_at: db_row.ended_at,
            error_message: None,
            failed_at_element_id: None,
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/v1/process-instances/{id}",
    params(
        ("id" = String, Path, description = "Process instance ID (UUID)")
    ),
    responses(
        (status = 200, description = "Process instance found", body = ProcessInstanceResponse),
        (status = 404, description = "Not found", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse),
    ),
    tag = "Process Instances"
)]
pub async fn get(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
) -> Result<Json<ProcessInstanceResponse>, ApiError> {
    let row = sqlx::query!(
        r#"
        SELECT
            pi.id, pi.process_definition_id, pi.process_definition_version,
            pi.business_key, pi.state, pi.variables,
            pi.active_element_ids, pi.created_at, pi.ended_at, pi.error_message,
            (
                SELECT t.element_id
                FROM tasks t
                WHERE t.process_instance_id = pi.id AND t.state = 'FAILED'
                ORDER BY t.completed_at DESC NULLS LAST
                LIMIT 1
            ) AS failed_at_element_id
        FROM process_instances pi
        WHERE pi.id = $1
        "#,
        id
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| {
        ApiError::not_found(
            codes::INSTANCE_NOT_FOUND,
            format!("Instance '{id}' not found"),
        )
    })?;

    Ok(Json(ProcessInstanceResponse {
        id: row.id,
        process_definition_id: row.process_definition_id,
        process_definition_version: row.process_definition_version,
        business_key: row.business_key,
        state: row.state,
        variables: row.variables,
        active_element_ids: row.active_element_ids,
        created_at: row.created_at,
        ended_at: row.ended_at,
        error_message: row.error_message,
        failed_at_element_id: row.failed_at_element_id,
    }))
}

pub async fn schedule_timer<'c, E>(
    executor: E,
    instance_id: &str,
    element_id: &str,
    definition: &orrery::model::TimerDefinition,
    event_gateway_group_id: Option<&str>,
) -> Result<(), ApiError>
where
    E: sqlx::Executor<'c, Database = sqlx::Postgres>,
{
    let due = crate::timer_eval::evaluate_due_at(definition)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let timer_id = uuid::Uuid::new_v4().to_string();
    let kind_str = match definition.kind {
        orrery::model::TimerKind::Duration => "duration",
        orrery::model::TimerKind::Date => "date",
        orrery::model::TimerKind::Cycle => "cycle",
    };
    sqlx::query!(
        "INSERT INTO scheduled_timers \
         (id, process_instance_id, element_id, due_at, expression, timer_kind, event_gateway_group_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
        timer_id,
        instance_id,
        element_id,
        due,
        definition.expression,
        kind_str,
        event_gateway_group_id,
    )
    .execute(executor)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn record_history<'c, E>(
    executor: E,
    instance_id: &str,
    element_id: &str,
    element_type: &str,
    event_type: &str,
    variables: &serde_json::Value,
    element_name: Option<&str>,
    ordering: i32,
) -> Result<(), ApiError>
where
    E: sqlx::Executor<'c, Database = sqlx::Postgres>,
{
    sqlx::query(
        r#"
        INSERT INTO execution_history
            (process_instance_id, element_id, element_type, event_type, variables_snapshot, element_name, ordering)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(instance_id)
    .bind(element_id)
    .bind(element_type)
    .bind(event_type)
    .bind(variables)
    .bind(element_name)
    .bind(ordering)
    .execute(executor)
    .await
    .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(())
}

pub async fn persist_visited_history(
    conn: &mut sqlx::PgConnection,
    instance_id: &str,
    visited: &[VisitedElement],
    variables_json: &serde_json::Value,
) -> Result<(), ApiError> {
    for (i, v) in visited.iter().enumerate() {
        let event_type = match &v.event {
            VisitEvent::Activated => "ELEMENT_ACTIVATED",
            VisitEvent::Completed => "ELEMENT_COMPLETED",
            VisitEvent::ErrorThrown => "ERROR_THROWN",
            VisitEvent::EscalationThrown => "ESCALATION_THROWN",
            VisitEvent::MessageThrown => "MESSAGE_THROWN",
            VisitEvent::LinkJumped => "LINK_JUMPED",
            VisitEvent::Terminated => "TERMINATED",
        };
        record_history(
            &mut *conn,
            instance_id,
            &v.element_id,
            &v.element_type,
            event_type,
            variables_json,
            v.element_name.as_deref(),
            i as i32,
        )
        .await?;
    }
    Ok(())
}

pub async fn create_task_for_element<'c, E>(
    executor: E,
    instance_id: &str,
    element_id: &str,
    variables: &serde_json::Value,
    max_retries: i32,
    topic: Option<&str>,
) -> Result<(), ApiError>
where
    E: sqlx::Executor<'c, Database = sqlx::Postgres>,
{
    let task_id = uuid::Uuid::new_v4().to_string();
    sqlx::query!(
        r#"
        INSERT INTO tasks (id, process_instance_id, element_id, element_type, state, variables, max_retries, topic)
        VALUES ($1, $2, $3, 'SERVICE_TASK', 'CREATED', $4, $5, $6)
        "#,
        task_id,
        instance_id,
        element_id,
        variables as &serde_json::Value,
        max_retries,
        topic as Option<&str>,
    )
    .execute(executor)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(())
}

pub async fn create_message_subscription<'c, E>(
    executor: E,
    process_instance_id: &str,
    element_id: &str,
    message_name: &str,
    correlation_key_value: Option<&str>,
    event_gateway_group_id: Option<&str>,
) -> Result<(), ApiError>
where
    E: sqlx::Executor<'c, Database = sqlx::Postgres>,
{
    let id = Uuid::new_v4().to_string();
    sqlx::query!(
        r#"
        INSERT INTO message_subscriptions
            (id, process_instance_id, element_id, message_name, correlation_key_value, event_gateway_group_id)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        id, process_instance_id, element_id, message_name, correlation_key_value, event_gateway_group_id,
    )
    .execute(executor)
    .await
    .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(())
}

pub async fn create_message_boundary_subscription<'c, E>(
    executor: E,
    process_instance_id: &str,
    element_id: &str,
    attached_to_element: &str,
    message_name: &str,
    correlation_key_value: Option<&str>,
    is_interrupting: bool,
) -> Result<(), ApiError>
where
    E: sqlx::Executor<'c, Database = sqlx::Postgres>,
{
    let id = Uuid::new_v4().to_string();
    sqlx::query!(
        r#"INSERT INTO message_boundary_subscriptions
            (id, process_instance_id, element_id, attached_to_element, message_name,
             correlation_key_value, is_interrupting)
        VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
        id,
        process_instance_id,
        element_id,
        attached_to_element,
        message_name,
        correlation_key_value,
        is_interrupting,
    )
    .execute(executor)
    .await
    .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(())
}

pub async fn create_signal_boundary_subscription(
    conn: &mut sqlx::PgConnection,
    process_instance_id: &str,
    element_id: &str,
    attached_to_element: &str,
    signal_ref: &str,
    is_interrupting: bool,
) -> Result<(), ApiError> {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO signal_boundary_subscriptions
            (id, process_instance_id, element_id, attached_to_element, signal_ref, is_interrupting)
        VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&id)
    .bind(process_instance_id)
    .bind(element_id)
    .bind(attached_to_element)
    .bind(signal_ref)
    .bind(is_interrupting)
    .execute(&mut *conn)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(())
}

pub async fn create_signal_subscription<'c, E>(
    executor: E,
    process_instance_id: &str,
    element_id: &str,
    signal_ref: &str,
    event_gateway_group_id: Option<&str>,
) -> Result<(), ApiError>
where
    E: sqlx::Executor<'c, Database = sqlx::Postgres>,
{
    let id = Uuid::new_v4().to_string();
    sqlx::query!(
        r#"
        INSERT INTO signal_subscriptions (id, process_instance_id, element_id, signal_ref, event_gateway_group_id)
        VALUES ($1, $2, $3, $4, $5)
        "#,
        id, process_instance_id, element_id, signal_ref, event_gateway_group_id,
    )
    .execute(executor)
    .await
    .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(())
}

#[derive(Serialize, ToSchema)]
pub struct HistoryEntryResponse {
    pub id: i64,
    pub element_id: String,
    pub element_name: Option<String>,
    pub element_type: String,
    pub event_type: String,
    pub variables_snapshot: Value,
    pub ordering: i32,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_history_level")]
    pub level: String,
}

fn default_history_level() -> String {
    "activity".to_string()
}

#[utoipa::path(
    get,
    path = "/v1/process-instances/{id}/history",
    params(
        ("id" = String, Path, description = "Process instance ID"),
        ("level" = Option<String>, Query, description = "History level: 'activity' (default, excludes gateways) or 'full'"),
    ),
    responses(
        (status = 200, description = "Execution history", body = Vec<HistoryEntryResponse>),
        (status = 404, description = "Not found", body = ErrorResponse),
    ),
    tag = "Process Instances"
)]
pub async fn get_history(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Vec<HistoryEntryResponse>>, ApiError> {
    let exists = sqlx::query_scalar!("SELECT 1 FROM process_instances WHERE id = $1", id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if exists.is_none() {
        return Err(ApiError::not_found(
            codes::INSTANCE_NOT_FOUND,
            format!("Instance '{id}' not found"),
        ));
    }

    let is_full = q.level == "full";
    let sql = if is_full {
        "SELECT id, element_id, element_name, element_type, event_type, variables_snapshot, ordering, occurred_at \
         FROM execution_history WHERE process_instance_id = $1 \
         ORDER BY occurred_at ASC, ordering ASC"
    } else {
        "SELECT id, element_id, element_name, element_type, event_type, variables_snapshot, ordering, occurred_at \
         FROM execution_history WHERE process_instance_id = $1 \
         AND element_type NOT IN ('ExclusiveGateway', 'ParallelGateway', 'InclusiveGateway', 'EventBasedGateway') \
         ORDER BY occurred_at ASC, ordering ASC"
    };

    let rows = sqlx::query(sql)
        .bind(&id)
        .fetch_all(&pool)
        .await
        .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    use sqlx::Row;
    Ok(Json(
        rows.into_iter()
            .map(|r| HistoryEntryResponse {
                id: r.get("id"),
                element_id: r.get("element_id"),
                element_name: r.get("element_name"),
                element_type: r.get("element_type"),
                event_type: r.get("event_type"),
                variables_snapshot: r.get("variables_snapshot"),
                ordering: r.get("ordering"),
                occurred_at: r.get("occurred_at"),
            })
            .collect(),
    ))
}

#[utoipa::path(
    post,
    path = "/v1/process-instances/{id}/cancel",
    params(("id" = String, Path, description = "Process instance ID")),
    responses(
        (status = 200, description = "Instance cancelled", body = ProcessInstanceResponse),
        (status = 404, description = "Not found", body = ErrorResponse),
        (status = 409, description = "Instance already in terminal state", body = ErrorResponse),
    ),
    tag = "Process Instances"
)]
pub async fn cancel(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
) -> Result<Json<ProcessInstanceResponse>, ApiError> {
    // Only cancel non-terminal instances
    let row = sqlx::query(
        "UPDATE process_instances \
         SET state = 'CANCELLED', ended_at = NOW(), active_element_ids = '[]'::jsonb, \
             variables = variables - '__join_counts__' - '__inclusive_join_counts__' \
         WHERE id = $1 AND state NOT IN ('COMPLETED', 'FAILED', 'CANCELLED') \
         RETURNING id, process_definition_id, process_definition_version, business_key, \
                   state, variables, active_element_ids, created_at, ended_at, error_message",
    )
    .bind(&id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match row {
        Some(r) => {
            use sqlx::Row;

            // Cancel any open tasks for this instance
            sqlx::query!(
                "UPDATE tasks SET state = 'CANCELLED', completed_at = NOW() \
                 WHERE process_instance_id = $1 AND state IN ('CREATED', 'CLAIMED')",
                id,
            )
            .execute(&pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            // Cancel open message subscriptions
            sqlx::query!(
                "UPDATE message_subscriptions SET consumed_at = NOW() \
                 WHERE process_instance_id = $1 AND consumed_at IS NULL",
                id,
            )
            .execute(&pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            // Cancel open signal subscriptions
            sqlx::query!(
                "UPDATE signal_subscriptions SET consumed_at = NOW() \
                 WHERE process_instance_id = $1 AND consumed_at IS NULL",
                id,
            )
            .execute(&pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            // Cancel open message boundary subscriptions
            sqlx::query!(
                "UPDATE message_boundary_subscriptions SET consumed_at = NOW() \
                 WHERE process_instance_id = $1 AND consumed_at IS NULL",
                id,
            )
            .execute(&pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            // Mark unfired timers as fired
            sqlx::query!(
                "UPDATE scheduled_timers SET fired = TRUE, fired_at = NOW() \
                 WHERE process_instance_id = $1 AND fired = FALSE",
                id,
            )
            .execute(&pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            Ok(Json(ProcessInstanceResponse {
                id: r.get("id"),
                process_definition_id: r.get("process_definition_id"),
                process_definition_version: r.get("process_definition_version"),
                business_key: r.get("business_key"),
                state: r.get("state"),
                variables: r.get("variables"),
                active_element_ids: r.get("active_element_ids"),
                created_at: r.get("created_at"),
                ended_at: r.get("ended_at"),
                error_message: r.get("error_message"),
                failed_at_element_id: None,
            }))
        }
        None => {
            // Check if instance exists at all vs. already in terminal state
            let exists = sqlx::query_scalar!("SELECT 1 FROM process_instances WHERE id = $1", id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            if exists.is_none() {
                Err(ApiError::not_found(
                    codes::INSTANCE_NOT_FOUND,
                    format!("Instance '{id}' not found"),
                ))
            } else {
                Err(ApiError::conflict(
                    codes::INSTANCE_TERMINAL,
                    format!("Instance '{id}' is already in a terminal state"),
                ))
            }
        }
    }
}

#[utoipa::path(
    post,
    path = "/v1/process-instances/{id}/retry",
    params(("id" = String, Path, description = "Process instance ID")),
    responses(
        (status = 200, description = "Instance retried", body = ProcessInstanceResponse),
        (status = 404, description = "Not found", body = ErrorResponse),
        (status = 409, description = "Instance not in FAILED state", body = ErrorResponse),
    ),
    tag = "Process Instances"
)]
pub async fn retry(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
) -> Result<Json<ProcessInstanceResponse>, ApiError> {
    // Load the failed instance
    let inst = sqlx::query!(
        "SELECT process_definition_id, process_definition_version, variables, active_element_ids, state \
         FROM process_instances WHERE id = $1",
        id,
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| ApiError::not_found(codes::INSTANCE_NOT_FOUND, format!("Instance '{id}' not found")))?;

    if inst.state != "FAILED" {
        return Err(ApiError::conflict(
            codes::INSTANCE_TERMINAL,
            format!("Instance '{id}' is not in FAILED state"),
        ));
    }

    // Load BPMN definition and rebuild engine to derive wait states
    let def_row = sqlx::query!(
        "SELECT bpmn_xml FROM process_definitions WHERE id = $1 AND version = $2",
        inst.process_definition_id,
        inst.process_definition_version,
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let definition = parse_bpmn(&def_row.bpmn_xml).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Parse error: {e}"),
        )
    })?;

    let vars: HashMap<String, Value> = serde_json::from_value(inst.variables)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let active_ids: Vec<String> = serde_json::from_value(inst.active_element_ids)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut engine = Engine::new(definition);
    let result = engine
        .resume(vars, active_ids)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Derive new state from active elements
    let state_str = derive_db_state(
        &result.active_elements,
        result.is_completed,
        result.is_failed,
    );
    let mut vars_to_save = result.variables.clone();
    save_engine_internals(&engine, &mut vars_to_save);
    let vars_json = serde_json::to_value(&vars_to_save)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let active_ids_json: Vec<String> = result
        .active_elements
        .iter()
        .map(|e| e.element_id.clone())
        .collect();
    let active_ids_val = serde_json::to_value(&active_ids_json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let no_datetime: Option<chrono::DateTime<chrono::Utc>> = None;
    let no_string: Option<String> = None;

    // Reset instance: clear error, set new state, reopen ended_at
    let row = sqlx::query!(
        r#"
        UPDATE process_instances
        SET state = $1, variables = $2, active_element_ids = $3,
            ended_at = $4, error_message = $5
        WHERE id = $6
        RETURNING id, process_definition_id, process_definition_version, business_key,
                  state, variables, active_element_ids, created_at, ended_at, error_message
        "#,
        state_str,
        vars_json,
        active_ids_val,
        no_datetime,
        no_string,
        id,
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Re-create side effects (tasks, timers, subscriptions) for active elements.
    // Use empty previous_ids so all active elements get fresh side effects.
    let previous_ids = std::collections::HashSet::new();
    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    create_side_effects_for_new_elements(
        &mut conn,
        &id,
        &result,
        &previous_ids,
        engine.definition(),
        &vars_json,
        0,
    )
    .await?;
    sync_event_subprocess_subscriptions(&mut conn, &id, &result.event_subprocess_subscriptions)
        .await?;

    Ok(Json(ProcessInstanceResponse {
        id: row.id,
        process_definition_id: row.process_definition_id,
        process_definition_version: row.process_definition_version,
        business_key: row.business_key,
        state: row.state,
        variables: row.variables,
        active_element_ids: row.active_element_ids,
        created_at: row.created_at,
        ended_at: row.ended_at,
        error_message: row.error_message,
        failed_at_element_id: None,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateVariablesRequest {
    pub variables: HashMap<String, Value>,
}

#[utoipa::path(
    put,
    path = "/v1/process-instances/{id}/variables",
    params(("id" = String, Path, description = "Instance ID")),
    request_body(content = UpdateVariablesRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Variables updated", body = ProcessInstanceResponse),
        (status = 404, description = "Not found", body = ErrorResponse),
    ),
    tag = "Process Instances"
)]
pub async fn update_variables(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
    Json(req): Json<UpdateVariablesRequest>,
) -> Result<Json<ProcessInstanceResponse>, ApiError> {
    let patch = serde_json::to_value(&req.variables)
        .map_err(|e| ApiError::bad_request(codes::INVALID_VARIABLES, e.to_string()))?;

    let r = sqlx::query!(
        r#"
        UPDATE process_instances
        SET variables = variables || $2
        WHERE id = $1
        RETURNING id, process_definition_id, process_definition_version, business_key, state, variables, active_element_ids,
                  created_at, ended_at, error_message
        "#,
        id,
        patch,
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match r {
        None => Err(ApiError::not_found(
            codes::INSTANCE_NOT_FOUND,
            "Instance not found",
        )),
        Some(ref r) => {
            // Propagate variable changes to any pending tasks so workers receive
            // the updated snapshot when they fetch-and-lock.
            sqlx::query!(
                "UPDATE tasks SET variables = variables || $1::jsonb
                 WHERE process_instance_id = $2
                   AND state IN ('CREATED', 'CLAIMED')",
                patch as serde_json::Value,
                id,
            )
            .execute(&pool)
            .await
            .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            Ok(Json(ProcessInstanceResponse {
                id: r.id.clone(),
                process_definition_id: r.process_definition_id.clone(),
                process_definition_version: r.process_definition_version,
                business_key: r.business_key.clone(),
                state: r.state.clone(),
                variables: r.variables.clone(),
                active_element_ids: r.active_element_ids.clone(),
                created_at: r.created_at,
                ended_at: r.ended_at,
                error_message: r.error_message.clone(),
                failed_at_element_id: None,
            }))
        }
    }
}

/// Start a new process instance triggered by a MessageStartEvent.
/// Returns the new instance ID on success.
pub async fn start_instance_for_message(
    pool: &PgPool,
    process_def_key: &str,
    process_def_version: i32,
    bpmn_xml: &str,
    initial_variables: HashMap<String, Value>,
    business_key: Option<&str>,
) -> Result<String, ApiError> {
    let definition = parse_bpmn(bpmn_xml).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Parse error: {e}"),
        )
    })?;

    let mut engine = Engine::new(definition);
    let result = engine
        .start(initial_variables)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let instance_id = Uuid::new_v4().to_string();
    let previous_ids = std::collections::HashSet::new();
    let active_ids: Vec<String> = result
        .active_elements
        .iter()
        .map(|e| e.element_id.clone())
        .collect();
    let state_str = derive_db_state(
        &result.active_elements,
        result.is_completed,
        result.is_failed,
    );
    let mut vars_to_save = result.variables.clone();
    save_engine_internals(&engine, &mut vars_to_save);
    let variables_json = serde_json::to_value(&vars_to_save).unwrap();
    let active_ids_json = serde_json::to_value(&active_ids).unwrap();
    let ended_at: Option<chrono::DateTime<chrono::Utc>> =
        result.is_completed.then(chrono::Utc::now);

    sqlx::query!(
        r#"
        INSERT INTO process_instances
            (id, process_definition_id, process_definition_version, business_key, state, variables, active_element_ids, ended_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
        instance_id,
        process_def_key,
        process_def_version,
        business_key,
        state_str,
        variables_json,
        active_ids_json,
        ended_at,
    )
    .execute(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    persist_visited_history(&mut conn, &instance_id, &result.visited, &variables_json).await?;
    create_side_effects_for_new_elements(
        &mut conn,
        &instance_id,
        &result,
        &previous_ids,
        engine.definition(),
        &variables_json,
        0,
    )
    .await?;
    sync_event_subprocess_subscriptions(
        &mut conn,
        &instance_id,
        &result.event_subprocess_subscriptions,
    )
    .await?;

    Ok(instance_id)
}

/// Start a new process instance from a timer start event (no HTTP context needed).
/// Used by the scheduler to create instances triggered by timer_start_definitions.
pub async fn start_instance_for_timer(
    pool: &PgPool,
    def_id: &str,
    def_version: i32,
    bpmn_xml: &str,
) -> Result<(), anyhow::Error> {
    let definition = parse_bpmn(bpmn_xml)?;
    let mut engine = Engine::new(definition);
    let result = engine.start(HashMap::new())?;

    let instance_id = Uuid::new_v4().to_string();
    let previous_ids = std::collections::HashSet::new();
    let active_ids: Vec<String> = result
        .active_elements
        .iter()
        .map(|e| e.element_id.clone())
        .collect();
    let state_str = derive_db_state(
        &result.active_elements,
        result.is_completed,
        result.is_failed,
    );
    let mut vars_to_save = result.variables.clone();
    save_engine_internals(&engine, &mut vars_to_save);
    let variables_json = serde_json::to_value(&vars_to_save)?;
    let active_ids_json = serde_json::to_value(&active_ids)?;
    let ended_at: Option<chrono::DateTime<chrono::Utc>> =
        result.is_completed.then(chrono::Utc::now);

    sqlx::query!(
        r#"
        INSERT INTO process_instances
            (id, process_definition_id, process_definition_version, state, variables, active_element_ids, ended_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        instance_id,
        def_id,
        def_version,
        state_str,
        variables_json,
        active_ids_json,
        ended_at,
    )
    .execute(pool)
    .await?;

    let mut conn = pool.acquire().await?;
    persist_visited_history(&mut conn, &instance_id, &result.visited, &variables_json)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    create_side_effects_for_new_elements(
        &mut conn,
        &instance_id,
        &result,
        &previous_ids,
        engine.definition(),
        &variables_json,
        0,
    )
    .await?;
    sync_event_subprocess_subscriptions(
        &mut conn,
        &instance_id,
        &result.event_subprocess_subscriptions,
    )
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    Ok(())
}

/// Start a new process instance from a signal start event.
/// Used by the signal broadcast handler when a matching signal_start_definition is found.
pub async fn start_instance_for_signal(
    pool: &PgPool,
    process_def_key: &str,
    process_def_version: i32,
    bpmn_xml: &str,
    initial_variables: HashMap<String, Value>,
) -> Result<String, ApiError> {
    let definition = parse_bpmn(bpmn_xml).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Parse error: {e}"),
        )
    })?;

    let mut engine = Engine::new(definition);
    let result = engine
        .start(initial_variables)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let instance_id = Uuid::new_v4().to_string();
    let previous_ids = std::collections::HashSet::new();
    let active_ids: Vec<String> = result
        .active_elements
        .iter()
        .map(|e| e.element_id.clone())
        .collect();
    let state_str = derive_db_state(
        &result.active_elements,
        result.is_completed,
        result.is_failed,
    );
    let mut vars_to_save = result.variables.clone();
    save_engine_internals(&engine, &mut vars_to_save);
    let variables_json = serde_json::to_value(&vars_to_save).unwrap();
    let active_ids_json = serde_json::to_value(&active_ids).unwrap();
    let ended_at: Option<chrono::DateTime<chrono::Utc>> =
        result.is_completed.then(chrono::Utc::now);

    sqlx::query!(
        r#"
        INSERT INTO process_instances
            (id, process_definition_id, process_definition_version, state, variables, active_element_ids, ended_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        instance_id,
        process_def_key,
        process_def_version,
        state_str,
        variables_json,
        active_ids_json,
        ended_at,
    )
    .execute(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    persist_visited_history(&mut conn, &instance_id, &result.visited, &variables_json).await?;
    create_side_effects_for_new_elements(
        &mut conn,
        &instance_id,
        &result,
        &previous_ids,
        engine.definition(),
        &variables_json,
        0,
    )
    .await?;
    sync_event_subprocess_subscriptions(
        &mut conn,
        &instance_id,
        &result.event_subprocess_subscriptions,
    )
    .await?;

    Ok(instance_id)
}
