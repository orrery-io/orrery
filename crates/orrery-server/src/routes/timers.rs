use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use sqlx::PgPool;

use orrery_types::{TimerResponse, UpdateTimerRequest};

use crate::errors::{codes, ApiError};

/// GET /v1/process-instances/{id}/timers
/// Returns all scheduled timers for a process instance, ordered by creation time.
pub async fn list(
    State(pool): State<PgPool>,
    Path(instance_id): Path<String>,
) -> Result<Json<Vec<TimerResponse>>, ApiError> {
    let rows = sqlx::query!(
        "SELECT id, element_id, timer_kind, expression, due_at, fired, fired_at, created_at \
         FROM scheduled_timers \
         WHERE process_instance_id = $1 \
         ORDER BY created_at ASC",
        instance_id,
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let timers = rows
        .into_iter()
        .map(|r| TimerResponse {
            id: r.id,
            element_id: r.element_id,
            kind: r.timer_kind,
            expression: r.expression,
            due_at: r.due_at,
            fired: r.fired,
            fired_at: r.fired_at,
            created_at: r.created_at,
        })
        .collect();

    Ok(Json(timers))
}

/// POST /v1/process-instances/{id}/timers/{timer_id}/fast-forward
/// Immediately fires a pending timer, advancing the process instance.
pub async fn fast_forward(
    State(pool): State<PgPool>,
    Path((instance_id, timer_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    // Look up the timer and verify it belongs to this instance and isn't already fired
    let timer = sqlx::query!(
        "SELECT id, process_instance_id, element_id \
         FROM scheduled_timers \
         WHERE id = $1 AND process_instance_id = $2 AND fired = FALSE",
        timer_id,
        instance_id,
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| {
        ApiError::not_found(codes::TIMER_NOT_FOUND, "Timer not found or already fired")
    })?;

    crate::scheduler::advance_timer(
        &pool,
        &timer.id,
        &timer.process_instance_id,
        &timer.element_id,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

/// PUT /v1/process-instances/{id}/timers/{timer_id}
/// Reschedule a pending timer to a new expression, re-evaluating due_at.
pub async fn update(
    State(pool): State<PgPool>,
    Path((instance_id, timer_id)): Path<(String, String)>,
    Json(req): Json<UpdateTimerRequest>,
) -> Result<Json<TimerResponse>, ApiError> {
    // Detect kind from expression format
    let kind = if req.expression.starts_with('R') || req.expression.contains('/') {
        orrery::model::TimerKind::Cycle
    } else if req.expression.contains('-') && req.expression.contains('T') {
        orrery::model::TimerKind::Date
    } else {
        orrery::model::TimerKind::Duration
    };
    let kind_str = match kind {
        orrery::model::TimerKind::Duration => "duration",
        orrery::model::TimerKind::Date => "date",
        orrery::model::TimerKind::Cycle => "cycle",
    };
    let definition = orrery::model::TimerDefinition {
        kind,
        expression: req.expression.clone(),
    };
    let new_due = crate::timer_eval::evaluate_due_at(&definition)
        .map_err(|e| ApiError::bad_request(codes::INVALID_TIMER_EXPR, e))?;

    let row = sqlx::query!(
        "UPDATE scheduled_timers \
         SET due_at = $1, expression = $2, timer_kind = $3 \
         WHERE id = $4 AND process_instance_id = $5 AND fired = FALSE \
         RETURNING id, element_id, timer_kind, expression, due_at, fired, fired_at, created_at",
        new_due,
        req.expression,
        kind_str,
        timer_id,
        instance_id,
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| {
        ApiError::not_found(codes::TIMER_NOT_FOUND, "Timer not found or already fired")
    })?;

    Ok(Json(TimerResponse {
        id: row.id,
        element_id: row.element_id,
        kind: row.timer_kind,
        expression: row.expression,
        due_at: row.due_at,
        fired: row.fired,
        fired_at: row.fired_at,
        created_at: row.created_at,
    }))
}
