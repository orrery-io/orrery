use std::time::Duration;

use sqlx::PgPool;
use tokio::time::interval;

use orrery::engine::Engine;
use orrery::parser::parse_bpmn;

use crate::routes::instances::{
    cancel_all_remaining_side_effects, cancel_sibling_subscriptions,
    create_side_effects_for_new_elements, derive_db_state, persist_visited_history,
    save_engine_internals, schedule_timer, sync_event_subprocess_subscriptions,
};

/// Fires due timers and expires stale locks every `tick` interval. Run in a background Tokio task.
pub async fn run(pool: PgPool, tick: Duration) {
    let mut ticker = interval(tick);
    loop {
        ticker.tick().await;
        if let Err(e) = fire_due_timers(&pool).await {
            tracing::error!("scheduler error: {e}");
        }
        if let Err(e) = fire_due_start_timers(&pool).await {
            tracing::error!("start timer error: {e}");
        }
        if let Err(e) = expire_stale_locks(&pool).await {
            tracing::error!("lock expiry error: {e}");
        }
    }
}

async fn fire_due_start_timers(pool: &PgPool) -> Result<(), anyhow::Error> {
    let due = sqlx::query!(
        "SELECT id, process_def_key, process_def_version, element_id, expression, timer_kind \
         FROM timer_start_definitions \
         WHERE enabled = TRUE AND next_due_at IS NOT NULL AND next_due_at <= NOW()"
    )
    .fetch_all(pool)
    .await?;

    for row in due {
        if let Err(e) = fire_start_timer(
            pool,
            &row.id,
            &row.process_def_key,
            row.process_def_version,
            &row.element_id,
            &row.expression,
            &row.timer_kind,
        )
        .await
        {
            tracing::warn!("failed to fire start timer {}: {e}", row.id);
        }
    }
    Ok(())
}

async fn fire_start_timer(
    pool: &PgPool,
    start_def_id: &str,
    process_def_key: &str,
    process_def_version: i32,
    _element_id: &str,
    expression: &str,
    timer_kind: &str,
) -> Result<(), anyhow::Error> {
    // Load the process definition BPMN
    let def_row = sqlx::query!(
        "SELECT id, bpmn_xml FROM process_definitions \
         WHERE id = $1 AND version = $2",
        process_def_key,
        process_def_version,
    )
    .fetch_one(pool)
    .await?;

    // Create a new process instance
    crate::routes::instances::start_instance_for_timer(
        pool,
        &def_row.id,
        process_def_version,
        &def_row.bpmn_xml,
    )
    .await?;

    // Update next_due_at for cycle timers or disable for one-shot timers
    if timer_kind == "cycle" {
        let def = orrery::model::TimerDefinition {
            kind: orrery::model::TimerKind::Cycle,
            expression: expression.to_string(),
        };
        let next = crate::timer_eval::evaluate_due_at(&def).map_err(|e| anyhow::anyhow!(e))?;
        sqlx::query!(
            "UPDATE timer_start_definitions SET next_due_at = $1 WHERE id = $2",
            next,
            start_def_id,
        )
        .execute(pool)
        .await?;
    } else {
        sqlx::query!(
            "UPDATE timer_start_definitions SET enabled = FALSE WHERE id = $1",
            start_def_id,
        )
        .execute(pool)
        .await?;
    }

    Ok(())
}

async fn fire_due_timers(pool: &PgPool) -> Result<(), anyhow::Error> {
    let timers = sqlx::query!(
        "SELECT id, process_instance_id, element_id \
         FROM scheduled_timers WHERE due_at <= NOW() AND fired = FALSE"
    )
    .fetch_all(pool)
    .await?;

    for timer in timers {
        if let Err(e) = advance_timer(
            pool,
            &timer.id,
            &timer.process_instance_id,
            &timer.element_id,
        )
        .await
        {
            tracing::warn!("failed to fire timer {}: {e}", timer.id);
        }
    }

    // Fire due timer event subprocess subscriptions
    let due_esp = sqlx::query(
        "SELECT id, process_instance_id, esp_id, is_interrupting
         FROM event_subprocess_subscriptions
         WHERE trigger_type = 'timer' AND due_at <= NOW()",
    )
    .fetch_all(pool)
    .await?;

    for row in due_esp {
        use sqlx::Row;
        let row_id: String = row.get("id");
        let instance_id: String = row.get("process_instance_id");
        let esp_id: String = row.get("esp_id");
        let is_interrupting: bool = row.get("is_interrupting");

        // Optimistic delete: skip if already gone
        let deleted = sqlx::query("DELETE FROM event_subprocess_subscriptions WHERE id = $1")
            .bind(&row_id)
            .execute(pool)
            .await?
            .rows_affected();
        if deleted == 0 {
            continue;
        }

        if let Err(e) = advance_event_subprocess(pool, &instance_id, &esp_id, is_interrupting).await
        {
            tracing::warn!("failed to fire timer ESP {esp_id} for instance {instance_id}: {e}");
        }
    }

    Ok(())
}

pub(crate) async fn advance_timer(
    pool: &PgPool,
    timer_id: &str,
    instance_id: &str,
    element_id: &str,
) -> Result<(), anyhow::Error> {
    // Load instance and definition before opening the transaction so the engine
    // work happens outside the DB round-trip critical path.
    let inst = sqlx::query!(
        "SELECT process_definition_id, process_definition_version, variables, active_element_ids FROM process_instances WHERE id = $1",
        instance_id
    )
    .fetch_one(pool)
    .await?;

    let def_row = sqlx::query!(
        "SELECT bpmn_xml FROM process_definitions WHERE id = $1 AND version = $2",
        inst.process_definition_id,
        inst.process_definition_version
    )
    .fetch_one(pool)
    .await?;

    // Look up the event_gateway_group_id before engine work
    let group_id = sqlx::query_scalar!(
        "SELECT event_gateway_group_id FROM scheduled_timers WHERE id = $1",
        timer_id,
    )
    .fetch_one(pool)
    .await?;

    // Rebuild engine and fire timer (pure computation, no DB I/O)
    let definition = parse_bpmn(&def_row.bpmn_xml)?;
    let vars: std::collections::HashMap<String, serde_json::Value> =
        serde_json::from_value(inst.variables)?;
    let active_ids: Vec<String> = serde_json::from_value(inst.active_element_ids)?;
    let previous_ids: std::collections::HashSet<String> = active_ids.iter().cloned().collect();

    let mut engine = Engine::new(definition.clone());
    engine.resume(vars, active_ids)?;
    // Check if this is a boundary timer (fires differently than intermediate timers)
    let is_boundary_timer = definition.elements.iter().any(
        |e| matches!(e, orrery::model::FlowElement::TimerBoundaryEvent(tb) if tb.id == element_id),
    );
    let result = match if is_boundary_timer {
        engine.fire_boundary_timer(element_id)
    } else {
        engine.fire_timer(element_id)
    } {
        Ok(r) => r,
        Err(e) => {
            // Engine failed (e.g. gateway has no matching condition).
            // Mark instance as FAILED and clean up so it doesn't get stuck.
            let error_msg = e.to_string();
            let mut tx = pool.begin().await?;

            sqlx::query!(
                "UPDATE process_instances SET state = 'FAILED', ended_at = NOW(), error_message = $1 WHERE id = $2",
                error_msg,
                instance_id,
            )
            .execute(&mut *tx)
            .await?;

            sqlx::query!(
                "UPDATE scheduled_timers SET fired = TRUE, fired_at = NOW() WHERE id = $1 AND fired = FALSE",
                timer_id,
            )
            .execute(&mut *tx)
            .await?;

            sqlx::query!(
                "UPDATE message_subscriptions SET consumed_at = NOW() WHERE process_instance_id = $1 AND consumed_at IS NULL",
                instance_id,
            )
            .execute(&mut *tx)
            .await?;

            sqlx::query!(
                "UPDATE signal_subscriptions SET consumed_at = NOW() WHERE process_instance_id = $1 AND consumed_at IS NULL",
                instance_id,
            )
            .execute(&mut *tx)
            .await?;

            sqlx::query!(
                "UPDATE scheduled_timers SET fired = TRUE, fired_at = NOW() WHERE process_instance_id = $1 AND fired = FALSE",
                instance_id,
            )
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;
            return Err(anyhow::anyhow!(error_msg));
        }
    };

    // Persist atomically: mark timer fired AND update instance state in one transaction.
    // Keeping both in the same transaction means a failed engine call (above) never
    // leaves the timer permanently marked as fired while the instance is stuck.
    let mut vars_to_save = result.variables.clone();
    save_engine_internals(&engine, &mut vars_to_save);
    let vars_json = serde_json::to_value(&vars_to_save)?;

    // If this timer is part of an EBG group, filter out cancelled sibling elements
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
        .await?;

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
    let active_ids_json = serde_json::to_value(&active_ids)?;
    let ended_at: Option<chrono::DateTime<chrono::Utc>> =
        result.is_completed.then(chrono::Utc::now);

    let mut tx = pool.begin().await?;

    // Mark timer fired inside the transaction (prevents double-firing via optimistic lock).
    let rows = sqlx::query!(
        "UPDATE scheduled_timers SET fired = TRUE, fired_at = NOW() \
         WHERE id = $1 AND fired = FALSE",
        timer_id,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    if rows == 0 {
        // Another worker already fired this timer — nothing to do.
        return Ok(());
    }

    sqlx::query!(
        "UPDATE process_instances SET state = $1, variables = $2, active_element_ids = $3, ended_at = $4 WHERE id = $5",
        state_str,
        vars_json,
        active_ids_json,
        ended_at,
        instance_id,
    )
    .execute(&mut *tx)
    .await?;

    // Cancel EBG sibling subscriptions if this is part of an event-based gateway group
    if let Some(ref gid) = group_id {
        cancel_sibling_subscriptions(&mut tx, element_id, gid)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    }

    // Persist visit history from the engine
    persist_visited_history(&mut tx, instance_id, &result.visited, &vars_json)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Create side effects for newly activated elements
    create_side_effects_for_new_elements(
        &mut tx,
        instance_id,
        &result,
        &previous_ids,
        engine.definition(),
        &vars_json,
        0,
    )
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    sync_event_subprocess_subscriptions(
        &mut tx,
        instance_id,
        &result.event_subprocess_subscriptions,
    )
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Re-schedule cycle timers that have remaining repetitions
    let timer_row = sqlx::query!(
        "SELECT timer_kind, expression FROM scheduled_timers WHERE id = $1",
        timer_id,
    )
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(row) = timer_row {
        if row.timer_kind == "cycle" {
            if let Some(expr) = row.expression {
                if let Some(next_expr) = crate::timer_eval::decrement_cycle_count(&expr) {
                    let next_def = orrery::model::TimerDefinition {
                        kind: orrery::model::TimerKind::Cycle,
                        expression: next_expr,
                    };
                    schedule_timer(&mut *tx, instance_id, element_id, &next_def, None).await?;
                }
            }
        }
    }

    tx.commit().await?;
    Ok(())
}

async fn advance_event_subprocess(
    pool: &PgPool,
    instance_id: &str,
    esp_id: &str,
    is_interrupting: bool,
) -> Result<(), anyhow::Error> {
    let inst = sqlx::query!(
        "SELECT process_definition_id, process_definition_version, variables, active_element_ids FROM process_instances WHERE id = $1",
        instance_id
    )
    .fetch_one(pool)
    .await?;

    let def_row = sqlx::query!(
        "SELECT bpmn_xml FROM process_definitions WHERE id = $1 AND version = $2",
        inst.process_definition_id,
        inst.process_definition_version
    )
    .fetch_one(pool)
    .await?;

    let definition = parse_bpmn(&def_row.bpmn_xml)?;
    let vars: std::collections::HashMap<String, serde_json::Value> =
        serde_json::from_value(inst.variables)?;
    let active_ids: Vec<String> = serde_json::from_value(inst.active_element_ids)?;
    let previous_ids: std::collections::HashSet<String> = active_ids.iter().cloned().collect();

    let mut engine = Engine::new(definition);
    engine.resume(vars, active_ids.clone())?;

    let result = engine
        .trigger_event_subprocess(esp_id, std::collections::HashMap::new())
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut vars_to_save = result.variables.clone();
    save_engine_internals(&engine, &mut vars_to_save);
    let vars_json = serde_json::to_value(&vars_to_save)?;
    let active_ids_json = serde_json::to_value(
        result
            .active_elements
            .iter()
            .map(|e| e.element_id.as_str())
            .collect::<Vec<_>>(),
    )?;
    let state_str = derive_db_state(
        &result.active_elements,
        result.is_completed,
        result.is_failed,
    );
    let ended_at: Option<chrono::DateTime<chrono::Utc>> =
        result.is_completed.then(chrono::Utc::now);

    let mut conn = pool.acquire().await?;

    sqlx::query(
        "UPDATE process_instances SET state = $1, variables = $2, active_element_ids = $3, ended_at = $4 WHERE id = $5"
    )
    .bind(state_str).bind(&vars_json).bind(&active_ids_json).bind(ended_at).bind(instance_id)
    .execute(&mut *conn).await?;

    persist_visited_history(&mut conn, instance_id, &result.visited, &vars_json)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if is_interrupting {
        cancel_all_remaining_side_effects(&mut conn, instance_id, None)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    }

    create_side_effects_for_new_elements(
        &mut conn,
        instance_id,
        &result,
        &previous_ids,
        engine.definition(),
        &vars_json,
        0,
    )
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    sync_event_subprocess_subscriptions(
        &mut conn,
        instance_id,
        &result.event_subprocess_subscriptions,
    )
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    Ok(())
}

async fn expire_stale_locks(pool: &PgPool) -> Result<(), anyhow::Error> {
    let count = sqlx::query!(
        "UPDATE tasks
         SET state = 'CREATED',
             claimed_by = NULL,
             claimed_at = NULL,
             locked_until = NULL
         WHERE state = 'CLAIMED'
           AND locked_until IS NOT NULL
           AND locked_until < NOW()"
    )
    .execute(pool)
    .await?
    .rows_affected();

    if count > 0 {
        tracing::info!("released {} expired external task locks", count);
    }
    Ok(())
}
