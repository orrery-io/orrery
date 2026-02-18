use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use std::collections::HashMap;
use utoipa::ToSchema;

use crate::errors::{codes, ApiError, ErrorResponse};
use crate::routes::instances::{
    cancel_all_remaining_side_effects, cancel_sibling_subscriptions,
    create_side_effects_for_new_elements, derive_db_state, persist_visited_history,
    save_engine_internals, start_instance_for_message, sync_event_subprocess_subscriptions,
};
use orrery::engine::Engine;
use orrery::model::FlowElement;
use orrery::parser::parse_bpmn;

#[derive(Deserialize, ToSchema)]
pub struct SendMessageRequest {
    /// The message name (matches message name in BPMN definitions)
    pub message_name: String,
    /// Match against the BPMN correlation key expression value stored on the subscription
    pub correlation_key: Option<String>,
    /// Target a specific process instance by ID
    pub process_instance_id: Option<String>,
    /// Match against the instance's business_key
    pub business_key: Option<String>,
    /// Variables to inject into the instance when it wakes up
    #[serde(default)]
    pub variables: HashMap<String, Value>,
}

#[derive(Serialize, ToSchema)]
pub struct SendMessageResponse {
    /// The process instance ID that was woken up (or newly created)
    pub process_instance_id: String,
    /// The new state of the instance
    pub instance_state: String,
}

#[utoipa::path(
    post,
    path = "/v1/messages",
    request_body(content = SendMessageRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Message correlated and instance advanced or started", body = SendMessageResponse),
        (status = 404, description = "No waiting instance or start definition matched", body = ErrorResponse),
        (status = 409, description = "Ambiguous correlation: multiple subscriptions match", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse),
    ),
    tag = "Events"
)]
pub async fn send_message(
    State(pool): State<PgPool>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, ApiError> {
    // --- Tier 1: Check message boundary subscriptions ---
    // Filters: correlation_key matches subscription expr value (or open subscriptions),
    // process_instance_id and business_key narrow by instance. All provided filters AND together.
    let boundary_subs = sqlx::query!(
        r#"
        SELECT mbs.id, mbs.process_instance_id, mbs.element_id, mbs.attached_to_element, mbs.is_interrupting
        FROM message_boundary_subscriptions mbs
        JOIN process_instances pi ON pi.id = mbs.process_instance_id
        WHERE mbs.message_name = $1
          AND mbs.consumed_at IS NULL
          AND ($2::text IS NULL OR mbs.correlation_key_value IS NULL OR mbs.correlation_key_value = $2)
          AND ($3::text IS NULL OR pi.id = $3)
          AND ($4::text IS NULL OR pi.business_key = $4)
        "#,
        req.message_name,
        req.correlation_key.as_deref(),
        req.process_instance_id.as_deref(),
        req.business_key.as_deref(),
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if boundary_subs.len() > 1 {
        return Err(ApiError::conflict(
            codes::CORRELATION_AMBIGUOUS,
            format!(
                "Ambiguous correlation: {} boundary subscriptions match message '{}'",
                boundary_subs.len(),
                req.message_name
            ),
        ));
    }

    if let Some(bsub) = boundary_subs.into_iter().next() {
        return handle_boundary_message(
            &pool,
            &bsub.process_instance_id,
            &bsub.id,
            &bsub.element_id,
            req.variables,
        )
        .await;
    }

    // --- Tier 2: Check instance (intermediate catch) subscriptions ---
    // Filters: correlation_key matches subscription expr value (or open subscriptions),
    // process_instance_id and business_key narrow by instance. All provided filters AND together.
    let subs = sqlx::query!(
        r#"
        SELECT ms.id, ms.process_instance_id, ms.element_id
        FROM message_subscriptions ms
        JOIN process_instances pi ON pi.id = ms.process_instance_id
        WHERE ms.message_name = $1
          AND ms.consumed_at IS NULL
          AND ($2::text IS NULL OR ms.correlation_key_value IS NULL OR ms.correlation_key_value = $2)
          AND ($3::text IS NULL OR pi.id = $3)
          AND ($4::text IS NULL OR pi.business_key = $4)
        "#,
        req.message_name,
        req.correlation_key.as_deref(),
        req.process_instance_id.as_deref(),
        req.business_key.as_deref(),
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if subs.len() > 1 {
        return Err(ApiError::conflict(
            codes::CORRELATION_AMBIGUOUS,
            format!(
                "Ambiguous correlation: {} subscriptions match message '{}'",
                subs.len(),
                req.message_name
            ),
        ));
    }

    if let Some(sub) = subs.into_iter().next() {
        return handle_instance_message(
            &pool,
            &sub.id,
            &sub.process_instance_id,
            &sub.element_id,
            req.variables,
        )
        .await;
    }

    // --- Tier 3: Check message start definitions ---
    let start_def = sqlx::query!(
        r#"
        SELECT process_def_key, process_def_version, element_id
        FROM message_start_definitions
        WHERE message_name = $1 AND enabled = TRUE
        LIMIT 1
        "#,
        req.message_name,
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if let Some(sdef) = start_def {
        let def_row = sqlx::query!(
            "SELECT bpmn_xml FROM process_definitions WHERE id = $1 AND version = $2",
            sdef.process_def_key,
            sdef.process_def_version,
        )
        .fetch_one(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        // Use the incoming business_key as the new instance's business key.
        let instance_id = start_instance_for_message(
            &pool,
            &sdef.process_def_key,
            sdef.process_def_version,
            &def_row.bpmn_xml,
            req.variables,
            req.business_key.as_deref(),
        )
        .await?;

        let inst = sqlx::query!(
            "SELECT state FROM process_instances WHERE id = $1",
            instance_id,
        )
        .fetch_one(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        return Ok(Json(SendMessageResponse {
            process_instance_id: instance_id,
            instance_state: inst.state,
        }));
    }

    // --- Tier 4: Check event subprocess subscriptions (message trigger) ---
    let esp_subs = sqlx::query(
        "SELECT id, process_instance_id, esp_id, is_interrupting
         FROM event_subprocess_subscriptions
         WHERE trigger_type = 'message'
           AND message_name = $1
           AND ($2::text IS NULL OR process_instance_id = $2)",
    )
    .bind(&req.message_name)
    .bind(req.process_instance_id.as_deref())
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if let Some(esp_row) = esp_subs.into_iter().next() {
        use sqlx::Row;
        let row_id: String = esp_row.get("id");
        let instance_id: String = esp_row.get("process_instance_id");
        let esp_id: String = esp_row.get("esp_id");
        let is_interrupting: bool = esp_row.get("is_interrupting");

        let inst = sqlx::query(
            "SELECT process_definition_id, process_definition_version, variables, active_element_ids FROM process_instances WHERE id = $1"
        )
        .bind(&instance_id)
        .fetch_one(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let def_row =
            sqlx::query("SELECT bpmn_xml FROM process_definitions WHERE id = $1 AND version = $2")
                .bind(inst.get::<String, _>("process_definition_id"))
                .bind(inst.get::<i32, _>("process_definition_version"))
                .fetch_one(&pool)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let definition =
            parse_bpmn(def_row.get::<String, _>("bpmn_xml").as_str()).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Parse error: {e}"),
                )
            })?;
        let saved_vars: HashMap<String, Value> =
            serde_json::from_value(inst.get::<Value, _>("variables")).unwrap_or_default();
        let active_ids: Vec<String> =
            serde_json::from_value(inst.get::<Value, _>("active_element_ids")).unwrap_or_default();

        let mut engine = Engine::new(definition);
        engine
            .resume(saved_vars, active_ids.clone())
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let result = engine
            .trigger_event_subprocess(&esp_id, req.variables.clone())
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let mut vars_to_save = result.variables.clone();
        save_engine_internals(&engine, &mut vars_to_save);
        let variables_json = serde_json::to_value(&vars_to_save).unwrap();
        let active_ids_json: Value = serde_json::to_value(
            result
                .active_elements
                .iter()
                .map(|e| e.element_id.as_str())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let state_str = derive_db_state(
            &result.active_elements,
            result.is_completed,
            result.is_failed,
        );
        let ended_at: Option<chrono::DateTime<chrono::Utc>> =
            result.is_completed.then(chrono::Utc::now);

        sqlx::query(
            "UPDATE process_instances SET state = $1, variables = $2, active_element_ids = $3, ended_at = $4 WHERE id = $5"
        )
        .bind(state_str).bind(&variables_json).bind(&active_ids_json).bind(ended_at).bind(&instance_id)
        .execute(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let mut conn = pool
            .acquire()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        persist_visited_history(&mut conn, &instance_id, &result.visited, &variables_json).await?;

        // For interrupting ESP: cancel old side effects BEFORE creating new ones
        if is_interrupting {
            cancel_all_remaining_side_effects(&mut conn, &instance_id, None).await?;
            sqlx::query("DELETE FROM event_subprocess_subscriptions WHERE id = $1")
                .bind(&row_id)
                .execute(&mut *conn)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }

        let previous_ids: std::collections::HashSet<String> = active_ids.into_iter().collect();
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

        let state: String = sqlx::query_scalar("SELECT state FROM process_instances WHERE id = $1")
            .bind(&instance_id)
            .fetch_one(&pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        return Ok(Json(SendMessageResponse {
            process_instance_id: instance_id,
            instance_state: state,
        }));
    }

    Err(ApiError::not_found(
        codes::MESSAGE_NO_MATCH,
        format!(
            "No waiting instance, boundary, or start definition found for message '{}'",
            req.message_name
        ),
    ))
}

/// Deliver a message to an intermediate catch event subscription.
async fn handle_instance_message(
    pool: &PgPool,
    subscription_id: &str,
    process_instance_id: &str,
    element_id: &str,
    variables: HashMap<String, Value>,
) -> Result<Json<SendMessageResponse>, ApiError> {
    let inst = sqlx::query!(
        "SELECT process_definition_id, process_definition_version, variables, active_element_ids FROM process_instances WHERE id = $1",
        process_instance_id,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let def_row = sqlx::query!(
        "SELECT bpmn_xml FROM process_definitions WHERE id = $1 AND version = $2",
        inst.process_definition_id,
        inst.process_definition_version,
    )
    .fetch_one(pool)
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
    let active_ids: Vec<String> = serde_json::from_value(inst.active_element_ids)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let previous_ids: std::collections::HashSet<String> = active_ids.iter().cloned().collect();

    // Look up the event_gateway_group_id before consuming the subscription
    let group_id = sqlx::query_scalar!(
        "SELECT event_gateway_group_id FROM message_subscriptions WHERE id = $1",
        subscription_id,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut engine = Engine::new(definition);
    engine
        .resume(instance_vars, active_ids)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let result = engine
        .receive_message(element_id, variables)
        .map_err(|e| ApiError::unprocessable(codes::ENGINE_REJECTED, e.to_string()))?;

    let mut vars_to_save = result.variables.clone();
    save_engine_internals(&engine, &mut vars_to_save);
    let variables_json = serde_json::to_value(&vars_to_save).unwrap();

    // If this subscription is part of an EBG group, filter out cancelled sibling elements
    let effective_elements: Vec<orrery::engine::ActiveElement> = if let Some(ref gid) = group_id {
        let cancelled = sqlx::query_scalar!(
            "SELECT element_id FROM message_subscriptions WHERE event_gateway_group_id = $1 AND element_id != $2 AND consumed_at IS NULL
             UNION ALL
             SELECT element_id FROM scheduled_timers WHERE event_gateway_group_id = $1 AND element_id != $2
             UNION ALL
             SELECT element_id FROM signal_subscriptions WHERE event_gateway_group_id = $1 AND element_id != $2 AND consumed_at IS NULL",
            gid, element_id,
        )
        .fetch_all(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let cancelled_set: std::collections::HashSet<String> =
            cancelled.into_iter().flatten().collect();
        result
            .active_elements
            .iter()
            .filter(|e| !cancelled_set.contains(&e.element_id))
            .cloned()
            .collect()
    } else {
        result.active_elements.clone()
    };

    let state_str =
        derive_db_state(&effective_elements, result.is_completed, result.is_failed).to_string();
    let active_ids: Vec<String> = effective_elements
        .iter()
        .map(|e| e.element_id.clone())
        .collect();
    let active_ids_json = serde_json::to_value(&active_ids).unwrap();
    let ended_at: Option<chrono::DateTime<chrono::Utc>> = if result.is_completed {
        Some(chrono::Utc::now())
    } else {
        None
    };

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    sqlx::query!(
        "UPDATE process_instances SET state = $1, variables = $2, active_element_ids = $3, ended_at = $4 WHERE id = $5",
        state_str, variables_json, active_ids_json, ended_at, process_instance_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    sqlx::query!(
        "UPDATE message_subscriptions SET consumed_at = NOW() WHERE id = $1",
        subscription_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Cancel EBG sibling subscriptions if this is part of an event-based gateway group
    if let Some(ref gid) = group_id {
        cancel_sibling_subscriptions(&mut tx, element_id, gid).await?;
    }

    // Cancel any lingering boundary subscriptions for this task element
    sqlx::query!(
        "UPDATE message_boundary_subscriptions \
         SET consumed_at = NOW() \
         WHERE attached_to_element = $1 AND process_instance_id = $2 AND consumed_at IS NULL",
        element_id,
        process_instance_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    persist_visited_history(
        &mut tx,
        process_instance_id,
        &result.visited,
        &variables_json,
    )
    .await?;

    create_side_effects_for_new_elements(
        &mut tx,
        process_instance_id,
        &result,
        &previous_ids,
        engine.definition(),
        &variables_json,
        0,
    )
    .await?;

    tx.commit()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(SendMessageResponse {
        process_instance_id: process_instance_id.to_string(),
        instance_state: state_str,
    }))
}

/// Deliver a message to a boundary event subscription on an active task.
async fn handle_boundary_message(
    pool: &PgPool,
    process_instance_id: &str,
    subscription_id: &str,
    boundary_element_id: &str,
    variables: HashMap<String, Value>,
) -> Result<Json<SendMessageResponse>, ApiError> {
    let inst = sqlx::query!(
        "SELECT process_definition_id, process_definition_version, variables, active_element_ids FROM process_instances WHERE id = $1",
        process_instance_id,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let def_row = sqlx::query!(
        "SELECT bpmn_xml FROM process_definitions WHERE id = $1 AND version = $2",
        inst.process_definition_id,
        inst.process_definition_version,
    )
    .fetch_one(pool)
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
    let active_ids: Vec<String> = serde_json::from_value(inst.active_element_ids)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let previous_ids: std::collections::HashSet<String> = active_ids.iter().cloned().collect();

    let mut engine = Engine::new(definition);
    engine
        .resume(instance_vars, active_ids)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let result = engine
        .receive_boundary_message(boundary_element_id, variables)
        .map_err(|e| ApiError::unprocessable(codes::ENGINE_REJECTED, e.to_string()))?;

    let state_str = derive_db_state(
        &result.active_elements,
        result.is_completed,
        result.is_failed,
    )
    .to_string();
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

    sqlx::query!(
        "UPDATE process_instances SET state = $1, variables = $2, active_element_ids = $3, ended_at = $4 WHERE id = $5",
        state_str, variables_json, active_ids_json, ended_at, process_instance_id,
    )
    .execute(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Mark the boundary subscription consumed
    sqlx::query!(
        "UPDATE message_boundary_subscriptions SET consumed_at = NOW() WHERE id = $1",
        subscription_id,
    )
    .execute(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // If interrupting boundary: cancel the task body's message subscription so it cannot match future messages
    let attached_and_interrupting = engine
        .definition()
        .elements
        .iter()
        .find(|e| e.id() == boundary_element_id)
        .and_then(|e| match e {
            FlowElement::MessageBoundaryEvent(mb) => {
                Some((mb.attached_to_ref.clone(), mb.is_interrupting))
            }
            _ => None,
        });

    if let Some((attached_to, true)) = attached_and_interrupting {
        sqlx::query!(
            "UPDATE message_subscriptions \
             SET consumed_at = NOW() \
             WHERE element_id = $1 AND process_instance_id = $2 AND consumed_at IS NULL",
            attached_to,
            process_instance_id,
        )
        .execute(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    persist_visited_history(
        &mut conn,
        process_instance_id,
        &result.visited,
        &variables_json,
    )
    .await?;
    create_side_effects_for_new_elements(
        &mut conn,
        process_instance_id,
        &result,
        &previous_ids,
        engine.definition(),
        &variables_json,
        0,
    )
    .await?;

    Ok(Json(SendMessageResponse {
        process_instance_id: process_instance_id.to_string(),
        instance_state: state_str,
    }))
}
