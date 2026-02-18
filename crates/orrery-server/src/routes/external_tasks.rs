use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use orrery::engine::Engine;
use orrery::parser::parse_bpmn;
use orrery_types::{
    CompleteExternalTaskRequest, ExtendLockRequest, ExternalTaskResponse, FailExternalTaskRequest,
    FetchAndLockRequest,
};

use crate::errors::{codes, ApiError};

#[derive(sqlx::FromRow)]
struct FetchedTaskRow {
    id: String,
    topic: Option<String>,
    process_instance_id: String,
    element_id: String,
    variables: serde_json::Value,
    claimed_by: Option<String>,
    locked_until: Option<chrono::DateTime<Utc>>,
    retry_count: i32,
    max_retries: i32,
    created_at: chrono::DateTime<Utc>,
}

use crate::routes::instances::{
    create_side_effects_for_new_elements, derive_db_state, persist_visited_history,
    save_engine_internals, sync_event_subprocess_subscriptions,
};

// ── fetch-and-lock ────────────────────────────────────────────────────────────

pub async fn fetch_and_lock(
    State(pool): State<PgPool>,
    Json(req): Json<FetchAndLockRequest>,
) -> Result<Json<Vec<ExternalTaskResponse>>, ApiError> {
    let timeout = Duration::from_millis(req.request_timeout_ms);
    let lock_duration = chrono::Duration::milliseconds(req.lock_duration_ms as i64);
    let deadline = Instant::now() + timeout;

    loop {
        let tasks = try_fetch_and_lock(&pool, &req, lock_duration).await?;
        if !tasks.is_empty() {
            return Ok(Json(tasks));
        }
        if Instant::now() >= deadline {
            return Ok(Json(vec![]));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn try_fetch_and_lock(
    pool: &PgPool,
    req: &FetchAndLockRequest,
    lock_duration: chrono::Duration,
) -> Result<Vec<ExternalTaskResponse>, ApiError> {
    let locked_until = Utc::now() + lock_duration;

    let subscriptions_json = serde_json::to_value(&req.subscriptions)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let rows = sqlx::query_as::<_, FetchedTaskRow>(
        r#"
        UPDATE tasks
        SET state = 'CLAIMED',
            claimed_by = $1,
            claimed_at = NOW(),
            locked_until = $2
        WHERE id IN (
            SELECT t.id FROM tasks t
            JOIN process_instances pi ON pi.id = t.process_instance_id
            WHERE t.state = 'CREATED'
              AND (t.locked_until IS NULL OR t.locked_until < NOW())
              AND EXISTS (
                  SELECT 1 FROM jsonb_array_elements($3) AS sub
                  WHERE t.topic = sub->>'topic'
                    AND (
                      jsonb_array_length(sub->'process_definition_ids') = 0
                      OR pi.process_definition_id = ANY(
                          ARRAY(SELECT jsonb_array_elements_text(sub->'process_definition_ids'))
                      )
                    )
              )
            ORDER BY t.created_at ASC
            LIMIT $4
            FOR UPDATE OF t SKIP LOCKED
        )
        RETURNING
            id, topic, process_instance_id,
            element_id, variables, claimed_by,
            locked_until, retry_count, max_retries, created_at
        "#,
    )
    .bind(req.worker_id.as_str())
    .bind(locked_until)
    .bind(&subscriptions_json)
    .bind(req.max_tasks as i64)
    .fetch_all(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let def_id = sqlx::query_scalar!(
            "SELECT process_definition_id FROM process_instances WHERE id = $1",
            row.process_instance_id
        )
        .fetch_one(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        result.push(ExternalTaskResponse {
            id: row.id,
            topic: row.topic.unwrap_or_default(),
            process_instance_id: row.process_instance_id,
            process_definition_id: def_id,
            element_id: row.element_id,
            variables: row.variables,
            worker_id: row.claimed_by.unwrap_or_default(),
            locked_until: row.locked_until.unwrap(),
            retry_count: row.retry_count,
            max_retries: row.max_retries,
            created_at: row.created_at,
        });
    }
    Ok(result)
}

// ── complete ──────────────────────────────────────────────────────────────────

pub async fn complete(
    State(pool): State<PgPool>,
    Path(task_id): Path<String>,
    Json(req): Json<CompleteExternalTaskRequest>,
) -> Result<Json<ExternalTaskResponse>, ApiError> {
    // 1. Load task, verify lock
    let task = sqlx::query!(
        "SELECT id, process_instance_id, element_id, state, claimed_by,
                topic, locked_until, retry_count, max_retries, created_at, variables
         FROM tasks WHERE id = $1",
        task_id
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| ApiError::not_found(codes::TASK_NOT_FOUND, "task not found"))?;

    if task.state != "CLAIMED" {
        return Err(ApiError::conflict(
            codes::TASK_STATE_CONFLICT,
            "task is not in CLAIMED state",
        ));
    }
    if task.claimed_by.as_deref() != Some(&req.worker_id) {
        return Err(ApiError::conflict(
            codes::TASK_WRONG_OWNER,
            "task is owned by a different worker",
        ));
    }
    if task.locked_until.map(|t| t < Utc::now()).unwrap_or(true) {
        return Err(ApiError::conflict(
            codes::TASK_LOCK_EXPIRED,
            "task lock has expired",
        ));
    }

    // 2. Load instance + definition
    let inst = sqlx::query!(
        "SELECT process_definition_id, process_definition_version, variables, active_element_ids FROM process_instances WHERE id = $1",
        task.process_instance_id
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let definition_xml = sqlx::query_scalar!(
        "SELECT bpmn_xml FROM process_definitions WHERE id = $1 AND version = $2",
        inst.process_definition_id,
        inst.process_definition_version,
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let definition = parse_bpmn(&definition_xml)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 3. Rebuild engine and complete the task
    let instance_vars: HashMap<String, Value> =
        serde_json::from_value(inst.variables).unwrap_or_default();
    let active_element_ids: Vec<String> =
        serde_json::from_value(inst.active_element_ids.clone()).unwrap_or_default();
    let previous_ids: std::collections::HashSet<String> =
        active_element_ids.iter().cloned().collect();
    let mut engine = Engine::new(definition);
    engine
        .resume(instance_vars, active_element_ids)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let result = engine
        .complete_task(&task.element_id, req.variables.clone())
        .map_err(|e| ApiError::unprocessable(codes::ENGINE_REJECTED, e.to_string()))?;

    // 4. Persist updated variables + state
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
    let ended_at = if result.is_completed {
        Some(Utc::now())
    } else {
        None
    };

    sqlx::query!(
        "UPDATE process_instances SET state = $1, variables = $2, active_element_ids = $3, ended_at = $4 WHERE id = $5",
        state_str, variables_json, active_ids_json, ended_at, task.process_instance_id,
    )
    .execute(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 5. Mark task completed
    sqlx::query!(
        "UPDATE tasks SET state = 'COMPLETED', completed_at = NOW() WHERE id = $1",
        task_id
    )
    .execute(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 6. Persist visit history and create follow-on tasks/timers/subscriptions
    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    persist_visited_history(
        &mut conn,
        &task.process_instance_id,
        &result.visited,
        &variables_json,
    )
    .await?;
    create_side_effects_for_new_elements(
        &mut conn,
        &task.process_instance_id,
        &result,
        &previous_ids,
        engine.definition(),
        &variables_json,
        0,
    )
    .await?;
    sync_event_subprocess_subscriptions(
        &mut conn,
        &task.process_instance_id,
        &result.event_subprocess_subscriptions,
    )
    .await?;

    Ok(Json(ExternalTaskResponse {
        id: task.id,
        topic: task.topic.unwrap_or_default(),
        process_instance_id: task.process_instance_id,
        process_definition_id: inst.process_definition_id,
        element_id: task.element_id,
        variables: serde_json::to_value(&req.variables).unwrap_or_default(),
        worker_id: req.worker_id,
        locked_until: task.locked_until.unwrap_or_else(Utc::now),
        retry_count: task.retry_count,
        max_retries: task.max_retries,
        created_at: task.created_at,
    }))
}

// ── failure ───────────────────────────────────────────────────────────────────

pub async fn failure(
    State(pool): State<PgPool>,
    Path(task_id): Path<String>,
    Json(req): Json<FailExternalTaskRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let task = sqlx::query!(
        "SELECT id, process_instance_id, element_id, state, claimed_by,
                retry_count, max_retries, topic, locked_until, created_at, variables
         FROM tasks WHERE id = $1",
        task_id
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| ApiError::not_found(codes::TASK_NOT_FOUND, "task not found"))?;

    if task.state != "CLAIMED" {
        return Err(ApiError::conflict(
            codes::TASK_STATE_CONFLICT,
            "task is not in CLAIMED state",
        ));
    }
    if task.claimed_by.as_deref() != Some(&req.worker_id) {
        return Err(ApiError::conflict(
            codes::TASK_WRONG_OWNER,
            "task is owned by a different worker",
        ));
    }

    let new_retry_count = task.retry_count + 1;

    if req.retries > 0 {
        // Put back in CREATED after retry_timeout_ms
        let retry_at = Utc::now() + chrono::Duration::milliseconds(req.retry_timeout_ms as i64);
        sqlx::query!(
            "UPDATE tasks SET state = 'CREATED', claimed_by = NULL,
             locked_until = NULL, claimed_at = NULL,
             retry_count = $1, max_retries = $2, next_retry_at = $3
             WHERE id = $4",
            new_retry_count,
            req.retries,
            retry_at,
            task_id,
        )
        .execute(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    } else {
        // No retries — fail the task and let the engine handle boundary events
        sqlx::query!(
            "UPDATE tasks SET state = 'FAILED', retry_count = $1 WHERE id = $2",
            new_retry_count,
            task_id
        )
        .execute(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let inst = sqlx::query!(
            "SELECT process_definition_id, process_definition_version, variables, active_element_ids FROM process_instances WHERE id = $1",
            task.process_instance_id
        )
        .fetch_one(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let definition_xml = sqlx::query_scalar!(
            "SELECT bpmn_xml FROM process_definitions WHERE id = $1 AND version = $2",
            inst.process_definition_id,
            inst.process_definition_version,
        )
        .fetch_one(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let definition = parse_bpmn(&definition_xml)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let variables: HashMap<String, Value> =
            serde_json::from_value(inst.variables).unwrap_or_default();
        let active_element_ids: Vec<String> =
            serde_json::from_value(inst.active_element_ids.clone()).unwrap_or_default();
        let previous_ids: std::collections::HashSet<String> =
            active_element_ids.iter().cloned().collect();
        let mut engine = Engine::new(definition);
        engine
            .resume(variables, active_element_ids)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let fail_result = engine.fail_task(&task.element_id, None);

        match fail_result {
            Ok(result) => {
                let state_str = derive_db_state(
                    &result.active_elements,
                    result.is_completed,
                    result.is_failed,
                );
                let active_ids: Vec<String> = result
                    .active_elements
                    .iter()
                    .map(|e| e.element_id.clone())
                    .collect();
                let active_ids_json = serde_json::to_value(&active_ids).unwrap();
                let vars_json = serde_json::to_value(&result.variables).unwrap();
                let ended_at = if result.is_failed {
                    Some(Utc::now())
                } else {
                    None
                };
                let error_msg: Option<String> = if result.is_failed {
                    Some(format!(
                        "Task '{}' failed: {}",
                        task.element_id, req.error_message
                    ))
                } else {
                    None
                };
                sqlx::query!(
                    "UPDATE process_instances SET state = $1, active_element_ids = $2, ended_at = $3, error_message = $4 WHERE id = $5",
                    state_str, active_ids_json, ended_at, error_msg, task.process_instance_id
                )
                .execute(&pool)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

                let mut conn = pool
                    .acquire()
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                persist_visited_history(
                    &mut conn,
                    &task.process_instance_id,
                    &result.visited,
                    &vars_json,
                )
                .await?;
                create_side_effects_for_new_elements(
                    &mut conn,
                    &task.process_instance_id,
                    &result,
                    &previous_ids,
                    engine.definition(),
                    &vars_json,
                    0,
                )
                .await?;
                sync_event_subprocess_subscriptions(
                    &mut conn,
                    &task.process_instance_id,
                    &result.event_subprocess_subscriptions,
                )
                .await?;
            }
            Err(_) => {
                let error_msg = format!("Task '{}' failed: {}", task.element_id, req.error_message);
                sqlx::query!(
                    "UPDATE process_instances SET state = 'FAILED', ended_at = NOW(), error_message = $1 WHERE id = $2",
                    error_msg, task.process_instance_id
                )
                .execute(&pool)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            }
        }
    }

    Ok(Json(
        serde_json::json!({ "id": task_id, "retry_count": new_retry_count }),
    ))
}

// ── extend-lock ───────────────────────────────────────────────────────────────

pub async fn extend_lock(
    State(pool): State<PgPool>,
    Path(task_id): Path<String>,
    Json(req): Json<ExtendLockRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let new_locked_until = Utc::now() + chrono::Duration::milliseconds(req.new_duration_ms as i64);

    let rows = sqlx::query!(
        "UPDATE tasks SET locked_until = $1
         WHERE id = $2 AND state = 'CLAIMED' AND claimed_by = $3
         RETURNING id",
        new_locked_until,
        task_id,
        req.worker_id,
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if rows.is_empty() {
        return Err(ApiError::not_found(
            codes::TASK_NOT_FOUND,
            "task not found or not owned by worker",
        ));
    }

    Ok(Json(
        serde_json::json!({ "id": task_id, "locked_until": new_locked_until }),
    ))
}

// ── list ──────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ExternalTaskListQuery {
    pub topic: Option<String>,
    pub state: Option<String>,
    pub worker_id: Option<String>,
}

pub async fn list(
    State(pool): State<PgPool>,
    Query(q): Query<ExternalTaskListQuery>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let rows = sqlx::query!(
        r#"
        SELECT t.id, t.topic, t.process_instance_id,
               pi.process_definition_id,
               t.element_id, t.state, t.claimed_by,
               t.variables, t.created_at, t.locked_until,
               t.retry_count, t.max_retries
        FROM tasks t
        JOIN process_instances pi ON pi.id = t.process_instance_id
        WHERE t.topic IS NOT NULL
          AND ($1::text IS NULL OR t.topic = $1)
          AND ($2::text IS NULL OR t.state = $2)
          AND ($3::text IS NULL OR t.claimed_by = $3)
        ORDER BY t.created_at DESC
        LIMIT 200
        "#,
        q.topic,
        q.state,
        q.worker_id,
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let result: Vec<_> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id,
                "topic": r.topic,
                "process_instance_id": r.process_instance_id,
                "process_definition_id": r.process_definition_id,
                "element_id": r.element_id,
                "state": r.state,
                "claimed_by": r.claimed_by,
                "variables": r.variables,
                "created_at": r.created_at,
                "locked_until": r.locked_until,
                "retry_count": r.retry_count,
                "max_retries": r.max_retries,
            })
        })
        .collect();

    Ok(Json(result))
}
