use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use std::collections::HashMap;
use utoipa::ToSchema;

use crate::errors::{ApiError, ErrorResponse};
use crate::routes::instances::{
    cancel_all_remaining_side_effects, cancel_sibling_subscriptions,
    create_side_effects_for_new_elements, derive_db_state, persist_visited_history,
    save_engine_internals, start_instance_for_signal, sync_event_subprocess_subscriptions,
};
use orrery::engine::Engine;
use orrery::model::FlowElement;
use orrery::parser::parse_bpmn;

#[derive(Deserialize, ToSchema)]
pub struct BroadcastSignalRequest {
    /// Variables to inject into all woken instances
    #[serde(default)]
    pub variables: HashMap<String, Value>,
}

#[derive(Serialize, ToSchema)]
pub struct BroadcastSignalResponse {
    /// Number of process instances that were woken by this signal
    pub woken_count: usize,
    /// Number of new instances started by signal start events
    pub started_count: usize,
    /// IDs of the instances that were woken
    pub process_instance_ids: Vec<String>,
}

#[utoipa::path(
    post,
    path = "/v1/signals/{name}",
    params(("name" = String, Path, description = "Signal name (matches signalRef in BPMN)")),
    request_body(content = BroadcastSignalRequest, content_type = "application/json"),
    responses(
        (status = 200, description = "Signal broadcast to all waiting instances", body = BroadcastSignalResponse),
        (status = 500, description = "Server error", body = ErrorResponse),
    ),
    tag = "Events"
)]
pub async fn broadcast_signal(
    State(pool): State<PgPool>,
    Path(signal_name): Path<String>,
    Json(req): Json<BroadcastSignalRequest>,
) -> Result<Json<BroadcastSignalResponse>, ApiError> {
    let mut woken_ids = Vec::new();
    let mut started_count: usize = 0;

    // --- Tier 1: Signal boundary subscriptions ---
    let boundary_rows = sqlx::query(
        "SELECT id, process_instance_id, element_id
         FROM signal_boundary_subscriptions
         WHERE signal_ref = $1 AND consumed_at IS NULL",
    )
    .bind(&signal_name)
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    for row in &boundary_rows {
        use sqlx::Row;
        let bsub_id: String = row.get("id");
        let bsub_instance_id: String = row.get("process_instance_id");
        let bsub_element_id: String = row.get("element_id");
        if let Ok(()) = handle_boundary_signal(
            &pool,
            &bsub_instance_id,
            &bsub_id,
            &bsub_element_id,
            req.variables.clone(),
        )
        .await
        {
            woken_ids.push(bsub_instance_id);
        }
    }

    // --- Tier 2: Intermediate catch event subscriptions ---
    let subs = sqlx::query!(
        r#"
        SELECT id, process_instance_id, element_id, event_gateway_group_id
        FROM signal_subscriptions
        WHERE signal_ref = $1 AND consumed_at IS NULL
        "#,
        signal_name,
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    for sub in subs {
        // Load instance and definition
        let inst_opt = sqlx::query!(
            "SELECT process_definition_id, process_definition_version, variables, active_element_ids FROM process_instances WHERE id = $1",
            sub.process_instance_id,
        )
        .fetch_optional(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let inst = match inst_opt {
            Some(i) => i,
            None => continue, // Instance deleted; skip
        };

        let def_row = sqlx::query!(
            "SELECT bpmn_xml FROM process_definitions WHERE id = $1 AND version = $2",
            inst.process_definition_id,
            inst.process_definition_version,
        )
        .fetch_one(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let definition = match parse_bpmn(&def_row.bpmn_xml) {
            Ok(d) => d,
            Err(_) => continue, // Skip unparseable definitions
        };

        let instance_vars: HashMap<String, Value> =
            serde_json::from_value(inst.variables).unwrap_or_default();
        let active_ids: Vec<String> =
            serde_json::from_value(inst.active_element_ids).unwrap_or_default();
        let previous_ids: std::collections::HashSet<String> = active_ids.iter().cloned().collect();

        let mut engine = Engine::new(definition);
        if engine.resume(instance_vars, active_ids).is_err() {
            continue;
        }

        let result = match engine.receive_signal(&sub.element_id, req.variables.clone()) {
            Ok(r) => r,
            Err(_) => continue,
        };

        // Persist updated state
        let mut vars_to_save = result.variables.clone();
        save_engine_internals(&engine, &mut vars_to_save);
        let variables_json = serde_json::to_value(&vars_to_save).unwrap();

        // If this subscription is part of an EBG group, filter out cancelled sibling elements
        let effective_elements: Vec<orrery::engine::ActiveElement> = if let Some(ref gid) =
            sub.event_gateway_group_id
        {
            let cancelled = sqlx::query_scalar!(
                "SELECT element_id FROM message_subscriptions WHERE event_gateway_group_id = $1 AND element_id != $2 AND consumed_at IS NULL
                 UNION ALL
                 SELECT element_id FROM scheduled_timers WHERE event_gateway_group_id = $1 AND element_id != $2
                 UNION ALL
                 SELECT element_id FROM signal_subscriptions WHERE event_gateway_group_id = $1 AND element_id != $2 AND consumed_at IS NULL",
                gid, sub.element_id,
            )
            .fetch_all(&pool)
            .await
            .unwrap_or_default();

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

        let state_str = derive_db_state(&effective_elements, result.is_completed, result.is_failed);
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

        let tx_result: Result<(), String> = async {
            let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

            sqlx::query!(
                "UPDATE process_instances SET state = $1, variables = $2, active_element_ids = $3, ended_at = $4 WHERE id = $5",
                state_str, variables_json, active_ids_json, ended_at, sub.process_instance_id,
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

            // Mark subscription consumed
            sqlx::query!(
                "UPDATE signal_subscriptions SET consumed_at = NOW() WHERE id = $1",
                sub.id,
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

            // Cancel EBG sibling subscriptions if this is part of an event-based gateway group
            if let Some(ref gid) = sub.event_gateway_group_id {
                cancel_sibling_subscriptions(&mut tx, &sub.element_id, gid)
                    .await
                    .map_err(|e| e.to_string())?;
            }

            // Persist visit history and create side effects for newly activated elements
            persist_visited_history(&mut tx, &sub.process_instance_id, &result.visited, &variables_json)
                .await.map_err(|e| e.to_string())?;
            create_side_effects_for_new_elements(
                &mut tx, &sub.process_instance_id, &result, &previous_ids, engine.definition(), &variables_json, 0,
            ).await.map_err(|e| e.to_string())?;

            tx.commit().await.map_err(|e| e.to_string())?;
            Ok(())
        }.await;

        if let Err(e) = tx_result {
            tracing::error!(
                "Failed to process signal for instance {}: {e}",
                sub.process_instance_id
            );
            continue;
        }

        woken_ids.push(sub.process_instance_id);
    }

    // --- Tier 3: Signal start definitions ---
    let start_def_rows = sqlx::query(
        "SELECT process_def_key, process_def_version
         FROM signal_start_definitions
         WHERE signal_ref = $1 AND enabled = TRUE",
    )
    .bind(&signal_name)
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    for srow in &start_def_rows {
        use sqlx::Row;
        let process_def_key: String = srow.get("process_def_key");
        let process_def_version: i32 = srow.get("process_def_version");

        let def_row = sqlx::query!(
            "SELECT bpmn_xml FROM process_definitions WHERE id = $1 AND version = $2",
            process_def_key,
            process_def_version,
        )
        .fetch_one(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        match start_instance_for_signal(
            &pool,
            &process_def_key,
            process_def_version,
            &def_row.bpmn_xml,
            req.variables.clone(),
        )
        .await
        {
            Ok(instance_id) => {
                woken_ids.push(instance_id);
                started_count += 1;
            }
            Err(e) => {
                tracing::error!(
                    "Failed to start signal instance for {}: {e}",
                    process_def_key
                );
            }
        }
    }

    // --- Tier 4: Signal event subprocess subscriptions ---
    let esp_subs = sqlx::query(
        "SELECT id, process_instance_id, esp_id, is_interrupting
         FROM event_subprocess_subscriptions
         WHERE trigger_type = 'signal' AND signal_ref = $1",
    )
    .bind(signal_name)
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    for esp_row in esp_subs {
        use sqlx::Row;
        let row_id: String = esp_row.get("id");
        let instance_id: String = esp_row.get("process_instance_id");
        let esp_id: String = esp_row.get("esp_id");
        let is_interrupting: bool = esp_row.get("is_interrupting");

        let inst = sqlx::query(
            "SELECT process_definition_id, process_definition_version, variables, active_element_ids FROM process_instances WHERE id = $1"
        ).bind(&instance_id).fetch_one(&pool).await
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

        let mut engine = orrery::engine::Engine::new(definition);
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
        .execute(&pool).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let mut conn = pool
            .acquire()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        persist_visited_history(&mut conn, &instance_id, &result.visited, &variables_json).await?;

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

        woken_ids.push(instance_id);
    }

    Ok(Json(BroadcastSignalResponse {
        woken_count: woken_ids.len(),
        started_count,
        process_instance_ids: woken_ids,
    }))
}

/// Handle a signal boundary event subscription on an active task.
async fn handle_boundary_signal(
    pool: &PgPool,
    process_instance_id: &str,
    subscription_id: &str,
    boundary_element_id: &str,
    variables: HashMap<String, Value>,
) -> Result<(), ApiError> {
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
        .receive_boundary_signal(boundary_element_id, variables)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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

    // Mark the boundary subscription consumed
    sqlx::query("UPDATE signal_boundary_subscriptions SET consumed_at = NOW() WHERE id = $1")
        .bind(subscription_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // If interrupting: cancel the task and its subscriptions
    let attached_and_interrupting = engine
        .definition()
        .elements
        .iter()
        .find(|e| e.id() == boundary_element_id)
        .and_then(|e| match e {
            FlowElement::SignalBoundaryEvent(sb) => {
                Some((sb.attached_to_ref.clone(), sb.is_interrupting))
            }
            _ => None,
        });

    if let Some((attached_to, true)) = attached_and_interrupting {
        // Cancel the attached task
        sqlx::query(
            "UPDATE tasks SET state = 'CANCELLED', completed_at = NOW() \
             WHERE element_id = $1 AND process_instance_id = $2 AND state IN ('CREATED', 'CLAIMED')",
        )
        .bind(&attached_to)
        .bind(process_instance_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        // Cancel other boundary subscriptions on the same task
        sqlx::query(
            "UPDATE signal_boundary_subscriptions SET consumed_at = NOW() \
             WHERE attached_to_element = $1 AND process_instance_id = $2 AND consumed_at IS NULL AND id != $3",
        )
        .bind(&attached_to)
        .bind(process_instance_id)
        .bind(subscription_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        sqlx::query!(
            "UPDATE message_boundary_subscriptions SET consumed_at = NOW() \
             WHERE attached_to_element = $1 AND process_instance_id = $2 AND consumed_at IS NULL",
            attached_to,
            process_instance_id,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

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
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(())
}
