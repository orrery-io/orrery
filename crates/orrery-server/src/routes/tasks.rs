use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use std::collections::HashMap;
use utoipa::ToSchema;

use crate::errors::{codes, ApiError, ErrorResponse};
use crate::routes::instances::{
    cancel_all_remaining_side_effects, create_side_effects_for_new_elements,
    create_task_for_element, derive_db_state, persist_visited_history, save_engine_internals,
    sync_event_subprocess_subscriptions,
};
use orrery::engine::Engine;
use orrery::parser::parse_bpmn;

#[derive(Serialize, ToSchema)]
pub struct TaskResponse {
    pub id: String,
    pub process_instance_id: String,
    pub process_definition_id: String,
    pub element_id: String,
    pub element_type: String,
    /// One of: CREATED, CLAIMED, COMPLETED, FAILED, CANCELLED
    pub state: String,
    pub claimed_by: Option<String>,
    pub variables: Value,
    pub created_at: DateTime<Utc>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub retry_count: i32,
    pub max_retries: i32,
}

#[derive(Deserialize, ToSchema)]
pub struct ClaimRequest {
    /// Identifier of the worker claiming this task (e.g. worker ID or username)
    pub claimed_by: String,
}

#[derive(Deserialize, ToSchema)]
pub struct CompleteTaskRequest {
    /// Output variables to merge into the process instance
    #[serde(default)]
    pub variables: HashMap<String, Value>,
}

#[derive(Deserialize, ToSchema)]
pub struct FailRequest {
    /// Human-readable failure reason
    pub reason: String,
}

#[derive(Deserialize)]
pub struct TaskListQuery {
    pub state: Option<String>,
    pub instance_id: Option<String>,
}

#[utoipa::path(
    get,
    path = "/v1/tasks",
    params(
        ("state" = Option<String>, Query, description = "Filter by state"),
        ("instance_id" = Option<String>, Query, description = "Filter by process instance ID"),
    ),
    responses(
        (status = 200, description = "List of tasks", body = Vec<TaskResponse>),
    ),
    tag = "Tasks"
)]
pub async fn list(
    State(pool): State<PgPool>,
    Query(q): Query<TaskListQuery>,
) -> Result<Json<Vec<TaskResponse>>, ApiError> {
    let rows = sqlx::query!(
        r#"
        SELECT t.id, t.process_instance_id, pi.process_definition_id,
               t.element_id, t.element_type, t.state,
               t.claimed_by, t.variables, t.created_at, t.claimed_at, t.completed_at,
               t.retry_count, t.max_retries
        FROM tasks t
        JOIN process_instances pi ON pi.id = t.process_instance_id
        WHERE ($1::text IS NULL OR t.state = $1)
          AND ($2::text IS NULL OR t.process_instance_id = $2)
        ORDER BY t.created_at ASC
        "#,
        q.state,
        q.instance_id,
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(
        rows.into_iter()
            .map(|r| TaskResponse {
                id: r.id,
                process_instance_id: r.process_instance_id,
                process_definition_id: r.process_definition_id,
                element_id: r.element_id,
                element_type: r.element_type,
                state: r.state,
                claimed_by: r.claimed_by,
                variables: r.variables,
                created_at: r.created_at,
                claimed_at: r.claimed_at,
                completed_at: r.completed_at,
                retry_count: r.retry_count,
                max_retries: r.max_retries,
            })
            .collect(),
    ))
}

#[utoipa::path(
    get,
    path = "/v1/tasks/{id}",
    params(("id" = String, Path, description = "Task ID")),
    responses(
        (status = 200, description = "Task found", body = TaskResponse),
        (status = 404, description = "Not found", body = ErrorResponse),
    ),
    tag = "Tasks"
)]
pub async fn get_task(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
) -> Result<Json<TaskResponse>, ApiError> {
    let r = sqlx::query!(
        r#"
        SELECT t.id, t.process_instance_id, pi.process_definition_id,
               t.element_id, t.element_type, t.state,
               t.claimed_by, t.variables, t.created_at, t.claimed_at, t.completed_at,
               t.retry_count, t.max_retries
        FROM tasks t
        JOIN process_instances pi ON pi.id = t.process_instance_id
        WHERE t.id = $1
        "#,
        id,
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| ApiError::not_found(codes::TASK_NOT_FOUND, format!("Task '{id}' not found")))?;

    Ok(Json(TaskResponse {
        id: r.id,
        process_instance_id: r.process_instance_id,
        process_definition_id: r.process_definition_id,
        element_id: r.element_id,
        element_type: r.element_type,
        state: r.state,
        claimed_by: r.claimed_by,
        variables: r.variables,
        created_at: r.created_at,
        claimed_at: r.claimed_at,
        completed_at: r.completed_at,
        retry_count: r.retry_count,
        max_retries: r.max_retries,
    }))
}

#[utoipa::path(
    post,
    path = "/v1/tasks/{id}/claim",
    params(("id" = String, Path, description = "Task ID")),
    request_body(content = ClaimRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Task claimed", body = TaskResponse),
        (status = 404, description = "Not found", body = ErrorResponse),
        (status = 409, description = "Task not in CREATED state", body = ErrorResponse),
    ),
    tag = "Tasks"
)]
pub async fn claim(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
    Json(req): Json<ClaimRequest>,
) -> Result<Json<TaskResponse>, ApiError> {
    let r = sqlx::query!(
        r#"
        UPDATE tasks
        SET state = 'CLAIMED', claimed_by = $1, claimed_at = NOW()
        WHERE id = $2 AND state = 'CREATED'
        RETURNING id, process_instance_id, element_id, element_type, state,
                  claimed_by, variables, created_at, claimed_at, completed_at,
                  retry_count, max_retries
        "#,
        req.claimed_by,
        id,
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match r {
        Some(r) => {
            let def_id = sqlx::query_scalar!(
                "SELECT process_definition_id FROM process_instances WHERE id = $1",
                r.process_instance_id,
            )
            .fetch_one(&pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            Ok(Json(TaskResponse {
                id: r.id,
                process_instance_id: r.process_instance_id,
                process_definition_id: def_id,
                element_id: r.element_id,
                element_type: r.element_type,
                state: r.state,
                claimed_by: r.claimed_by,
                variables: r.variables,
                created_at: r.created_at,
                claimed_at: r.claimed_at,
                completed_at: r.completed_at,
                retry_count: r.retry_count,
                max_retries: r.max_retries,
            }))
        }
        None => {
            let exists = sqlx::query_scalar!("SELECT 1 FROM tasks WHERE id = $1", id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            if exists.is_none() {
                Err(ApiError::not_found(
                    codes::TASK_NOT_FOUND,
                    format!("Task '{id}' not found"),
                ))
            } else {
                Err(ApiError::conflict(
                    codes::TASK_STATE_CONFLICT,
                    format!("Task '{id}' is not in CREATED state"),
                ))
            }
        }
    }
}

#[utoipa::path(
    post,
    path = "/v1/tasks/{id}/complete",
    params(("id" = String, Path, description = "Task ID")),
    request_body(content = CompleteTaskRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Task completed, instance advanced", body = TaskResponse),
        (status = 404, description = "Not found", body = ErrorResponse),
        (status = 409, description = "Task not in CLAIMED state", body = ErrorResponse),
        (status = 422, description = "Engine rejected completion", body = ErrorResponse),
    ),
    tag = "Tasks"
)]
pub async fn complete(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
    Json(req): Json<CompleteTaskRequest>,
) -> Result<Json<TaskResponse>, ApiError> {
    // 1. Load task, verify CLAIMED
    let task = sqlx::query!(
        "SELECT id, process_instance_id, element_id, state, variables FROM tasks WHERE id = $1",
        id,
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| ApiError::not_found(codes::TASK_NOT_FOUND, format!("Task '{id}' not found")))?;

    if task.state != "CLAIMED" {
        return Err(ApiError::conflict(
            codes::TASK_STATE_CONFLICT,
            format!(
                "Task '{}' is in state '{}', expected CLAIMED",
                id, task.state
            ),
        ));
    }

    // 2. Load instance + definition
    let inst = sqlx::query!(
        "SELECT process_definition_id, process_definition_version, variables, active_element_ids FROM process_instances WHERE id = $1",
        task.process_instance_id,
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let def_row = sqlx::query!(
        "SELECT bpmn_xml FROM process_definitions WHERE id = $1 AND version = $2",
        inst.process_definition_id,
        inst.process_definition_version,
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 3. Rebuild engine state
    let definition = parse_bpmn(&def_row.bpmn_xml).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Parse error: {e}"),
        )
    })?;

    let instance_vars: HashMap<String, Value> = serde_json::from_value(inst.variables)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let active_ids: Vec<String> = serde_json::from_value(inst.active_element_ids)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let previous_ids: std::collections::HashSet<String> = active_ids.iter().cloned().collect();

    let mut engine = Engine::new(definition);
    engine
        .resume(instance_vars, active_ids)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 4. Complete the task
    let result = engine
        .complete_task(&task.element_id, req.variables)
        .map_err(|e| ApiError::unprocessable(codes::ENGINE_REJECTED, e.to_string()))?;
    let state_str = derive_db_state(
        &result.active_elements,
        result.is_completed,
        result.is_failed,
    );
    let mut vars_to_save = result.variables.clone();
    save_engine_internals(&engine, &mut vars_to_save);
    let variables_json = serde_json::to_value(&vars_to_save).unwrap();
    let active_ids: Vec<String> = result
        .active_elements
        .iter()
        .map(|e| e.element_id.clone())
        .collect();
    let active_ids_json = serde_json::to_value(&active_ids).unwrap();
    let ended_at: Option<chrono::DateTime<chrono::Utc>> = if result.is_completed {
        Some(chrono::Utc::now())
    } else {
        None
    };

    // 5-7. Atomically update instance state, complete the task, and schedule side-effects
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    sqlx::query!(
        "UPDATE process_instances SET state = $1, variables = $2, active_element_ids = $3, ended_at = $4 WHERE id = $5",
        state_str, variables_json, active_ids_json, ended_at, task.process_instance_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 6. Update task to COMPLETED
    let updated = sqlx::query!(
        r#"
        UPDATE tasks SET state = 'COMPLETED', completed_at = NOW()
        WHERE id = $1
        RETURNING id, process_instance_id, element_id, element_type, state,
                  claimed_by, variables, created_at, claimed_at, completed_at,
                  retry_count, max_retries
        "#,
        id,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 7. Consume boundary subscriptions for the completed task
    sqlx::query!(
        "UPDATE message_boundary_subscriptions
         SET consumed_at = NOW()
         WHERE attached_to_element = $1
           AND process_instance_id = $2
           AND consumed_at IS NULL",
        task.element_id,
        task.process_instance_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 7a-bis. Consume signal boundary subscriptions for the completed task
    sqlx::query(
        "UPDATE signal_boundary_subscriptions
         SET consumed_at = NOW()
         WHERE attached_to_element = $1
           AND process_instance_id = $2
           AND consumed_at IS NULL",
    )
    .bind(&task.element_id)
    .bind(&task.process_instance_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 7b. Cancel boundary timers attached to the completed task
    let boundary_timer_ids: Vec<String> = engine
        .definition()
        .elements
        .iter()
        .filter_map(|e| {
            if let orrery::model::FlowElement::TimerBoundaryEvent(tb) = e {
                if tb.attached_to_ref == task.element_id {
                    return Some(tb.id.clone());
                }
            }
            None
        })
        .collect();
    if !boundary_timer_ids.is_empty() {
        sqlx::query(
            "UPDATE scheduled_timers SET fired = TRUE, fired_at = NOW() \
             WHERE process_instance_id = $1 AND fired = FALSE \
             AND element_id = ANY($2)",
        )
        .bind(&task.process_instance_id)
        .bind(&boundary_timer_ids)
        .execute(&mut *tx)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    // 8. Persist visit history from the engine
    persist_visited_history(
        &mut tx,
        &task.process_instance_id,
        &result.visited,
        &variables_json,
    )
    .await?;

    // 9. Create next task or schedule timer for each NEW active element
    create_side_effects_for_new_elements(
        &mut tx,
        &task.process_instance_id,
        &result,
        &previous_ids,
        engine.definition(),
        &variables_json,
        0,
    )
    .await?;
    sync_event_subprocess_subscriptions(
        &mut tx,
        &task.process_instance_id,
        &result.event_subprocess_subscriptions,
    )
    .await?;

    // 10. If instance completed (e.g. via TerminateEndEvent), cancel remaining side effects
    if result.is_completed {
        cancel_all_remaining_side_effects(&mut tx, &task.process_instance_id, Some(&id)).await?;
    }

    tx.commit()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let def_id = inst.process_definition_id;

    Ok(Json(TaskResponse {
        id: updated.id,
        process_instance_id: updated.process_instance_id,
        process_definition_id: def_id,
        element_id: updated.element_id,
        element_type: updated.element_type,
        state: updated.state,
        claimed_by: updated.claimed_by,
        variables: updated.variables,
        created_at: updated.created_at,
        claimed_at: updated.claimed_at,
        completed_at: updated.completed_at,
        retry_count: updated.retry_count,
        max_retries: updated.max_retries,
    }))
}

#[utoipa::path(
    post,
    path = "/v1/tasks/{id}/retry",
    params(("id" = String, Path, description = "Task ID")),
    responses(
        (status = 200, description = "Task reset to CREATED", body = TaskResponse),
        (status = 404, description = "Not found or not in FAILED state", body = ErrorResponse),
    ),
    tag = "Tasks"
)]
pub async fn retry(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
) -> Result<Json<TaskResponse>, ApiError> {
    let r = sqlx::query!(
        r#"
        UPDATE tasks
        SET state = 'CREATED',
            retry_count = retry_count + 1,
            completed_at = NULL::timestamptz,
            claimed_by = NULL::text,
            claimed_at = NULL::timestamptz,
            variables = (SELECT variables FROM process_instances WHERE id = process_instance_id)
        WHERE id = $1 AND state = 'FAILED'
        RETURNING id, process_instance_id, element_id, element_type, state,
                  claimed_by, variables, created_at, claimed_at, completed_at,
                  retry_count, max_retries
        "#,
        id,
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match r {
        None => Err(ApiError::not_found(
            codes::TASK_NOT_FOUND,
            "Task not found or not in FAILED state",
        )),
        Some(r) => {
            // Restore the instance to WAITING_FOR_TASK, clear the failure fields,
            // and put the element back in active_element_ids so polling/diagram work.
            let active_ids_json: serde_json::Value = serde_json::json!([r.element_id]);
            let no_datetime: Option<chrono::DateTime<chrono::Utc>> = None;
            let no_string: Option<String> = None;
            sqlx::query!(
                "UPDATE process_instances
                 SET state = 'WAITING_FOR_TASK',
                     ended_at = $1,
                     error_message = $2,
                     active_element_ids = $3
                 WHERE id = $4",
                no_datetime,
                no_string,
                active_ids_json,
                r.process_instance_id,
            )
            .execute(&pool)
            .await
            .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            let def_id = sqlx::query_scalar!(
                "SELECT process_definition_id FROM process_instances WHERE id = $1",
                r.process_instance_id,
            )
            .fetch_one(&pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            Ok(Json(TaskResponse {
                id: r.id,
                process_instance_id: r.process_instance_id,
                process_definition_id: def_id,
                element_id: r.element_id,
                element_type: r.element_type,
                state: r.state,
                claimed_by: r.claimed_by,
                variables: r.variables,
                created_at: r.created_at,
                claimed_at: r.claimed_at,
                completed_at: r.completed_at,
                retry_count: r.retry_count,
                max_retries: r.max_retries,
            }))
        }
    }
}

#[utoipa::path(
    post,
    path = "/v1/tasks/{id}/fail",
    params(("id" = String, Path, description = "Task ID")),
    request_body(content = FailRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Task failed", body = TaskResponse),
        (status = 404, description = "Not found", body = ErrorResponse),
        (status = 409, description = "Task already in terminal state", body = ErrorResponse),
    ),
    tag = "Tasks"
)]
pub async fn fail(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
    Json(req): Json<FailRequest>,
) -> Result<Json<TaskResponse>, ApiError> {
    let r = sqlx::query!(
        r#"
        UPDATE tasks
        SET state = 'FAILED', completed_at = NOW(), retry_count = retry_count + 1
        WHERE id = $1 AND state = 'CLAIMED'
        RETURNING id, process_instance_id, element_id, element_type, state,
                  claimed_by, variables, created_at, claimed_at, completed_at,
                  retry_count, max_retries, topic
        "#,
        id,
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match r {
        Some(r) => {
            if r.retry_count <= r.max_retries {
                // Retries remain — create a new task with remaining budget, keep instance waiting
                let remaining = r.max_retries - r.retry_count;
                create_task_for_element(
                    &pool,
                    &r.process_instance_id,
                    &r.element_id,
                    &r.variables,
                    remaining,
                    r.topic.as_deref(),
                )
                .await?;
            } else {
                // No retries left — use engine to check for boundary event
                let inst = sqlx::query!(
                    "SELECT process_definition_id, process_definition_version, variables, active_element_ids FROM process_instances WHERE id = $1",
                    r.process_instance_id,
                )
                .fetch_one(&pool)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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

                let instance_vars: HashMap<String, Value> = serde_json::from_value(inst.variables)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

                let active_element_ids: Vec<String> =
                    serde_json::from_value(inst.active_element_ids)
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

                let mut engine = Engine::new(definition);
                engine.variables = instance_vars;
                engine.tokens = active_element_ids
                    .iter()
                    .map(|id| orrery::engine::Token {
                        element_id: id.clone(),
                    })
                    .collect();

                let fail_result = engine
                    .fail_task(&r.element_id, None)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

                let new_state = derive_db_state(
                    &fail_result.active_elements,
                    fail_result.is_completed,
                    fail_result.is_failed,
                );

                let active_ids: Vec<String> = fail_result
                    .active_elements
                    .iter()
                    .map(|e| e.element_id.clone())
                    .collect();
                let active_ids_json = serde_json::to_value(&active_ids).unwrap();
                let mut vars_to_save = fail_result.variables.clone();
                save_engine_internals(&engine, &mut vars_to_save);
                let vars_json = serde_json::to_value(&vars_to_save).unwrap();
                let ended_at = if fail_result.is_failed {
                    Some(chrono::Utc::now())
                } else {
                    None
                };

                let error_msg = if fail_result.is_failed {
                    Some(format!("Task '{}' failed: {}", r.element_id, req.reason))
                } else {
                    None
                };

                sqlx::query!(
                    "UPDATE process_instances SET state = $1, variables = $2, active_element_ids = $3, ended_at = $4, error_message = $5 WHERE id = $6",
                    new_state, vars_json, active_ids_json, ended_at, error_msg, r.process_instance_id,
                )
                .execute(&pool)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

                // Consume boundary subscriptions for the failed task
                sqlx::query!(
                    "UPDATE message_boundary_subscriptions
                     SET consumed_at = NOW()
                     WHERE attached_to_element = $1
                       AND process_instance_id = $2
                       AND consumed_at IS NULL",
                    r.element_id,
                    r.process_instance_id,
                )
                .execute(&pool)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

                // Consume signal boundary subscriptions for the failed task
                sqlx::query(
                    "UPDATE signal_boundary_subscriptions
                     SET consumed_at = NOW()
                     WHERE attached_to_element = $1
                       AND process_instance_id = $2
                       AND consumed_at IS NULL",
                )
                .bind(&r.element_id)
                .bind(&r.process_instance_id)
                .execute(&pool)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

                // Cancel boundary timers attached to the failed task
                let boundary_timer_ids: Vec<String> = engine
                    .definition()
                    .elements
                    .iter()
                    .filter_map(|e| {
                        if let orrery::model::FlowElement::TimerBoundaryEvent(tb) = e {
                            if tb.attached_to_ref == r.element_id {
                                return Some(tb.id.clone());
                            }
                        }
                        None
                    })
                    .collect();
                if !boundary_timer_ids.is_empty() {
                    sqlx::query(
                        "UPDATE scheduled_timers SET fired = TRUE, fired_at = NOW() \
                         WHERE process_instance_id = $1 AND fired = FALSE \
                         AND element_id = ANY($2)",
                    )
                    .bind(&r.process_instance_id)
                    .bind(&boundary_timer_ids)
                    .execute(&pool)
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                }

                // Persist visit history from the engine
                {
                    let mut conn = pool
                        .acquire()
                        .await
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                    persist_visited_history(
                        &mut conn,
                        &r.process_instance_id,
                        &fail_result.visited,
                        &vars_json,
                    )
                    .await?;
                }

                // If boundary routed to new elements, create side effects
                if !fail_result.active_elements.is_empty()
                    || !fail_result.event_subprocess_subscriptions.is_empty()
                {
                    let prev: std::collections::HashSet<String> =
                        active_element_ids.into_iter().collect();
                    let mut conn = pool
                        .acquire()
                        .await
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                    create_side_effects_for_new_elements(
                        &mut conn,
                        &r.process_instance_id,
                        &fail_result,
                        &prev,
                        engine.definition(),
                        &vars_json,
                        0,
                    )
                    .await?;
                    sync_event_subprocess_subscriptions(
                        &mut conn,
                        &r.process_instance_id,
                        &fail_result.event_subprocess_subscriptions,
                    )
                    .await?;
                }
            }

            let fail_def_id = sqlx::query_scalar!(
                "SELECT process_definition_id FROM process_instances WHERE id = $1",
                r.process_instance_id,
            )
            .fetch_one(&pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            Ok(Json(TaskResponse {
                id: r.id,
                process_instance_id: r.process_instance_id,
                process_definition_id: fail_def_id,
                element_id: r.element_id,
                element_type: r.element_type,
                state: r.state,
                claimed_by: r.claimed_by,
                variables: r.variables,
                created_at: r.created_at,
                claimed_at: r.claimed_at,
                completed_at: r.completed_at,
                retry_count: r.retry_count,
                max_retries: r.max_retries,
            }))
        }
        None => {
            let exists = sqlx::query_scalar!("SELECT 1 FROM tasks WHERE id = $1", id)
                .fetch_optional(&pool)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            if exists.is_none() {
                Err(ApiError::not_found(
                    codes::TASK_NOT_FOUND,
                    format!("Task '{id}' not found"),
                ))
            } else {
                Err(ApiError::conflict(
                    codes::TASK_STATE_CONFLICT,
                    format!("Task '{id}' is not in CLAIMED state"),
                ))
            }
        }
    }
}
