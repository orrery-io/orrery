use crate::errors::ApiError;
use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;
use sqlx::PgPool;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct OverviewMetricsResponse {
    pub running_instances: i64,
    pub waiting_instances: i64,
    pub completed_instances: i64,
    pub failed_instances: i64,
    pub pending_tasks: i64,
    pub claimed_tasks: i64,
}

#[utoipa::path(
    get,
    path = "/v1/metrics/overview",
    responses(
        (status = 200, description = "Aggregate counts", body = OverviewMetricsResponse),
    ),
    tag = "Metrics"
)]
pub async fn overview(
    State(pool): State<PgPool>,
) -> Result<Json<OverviewMetricsResponse>, ApiError> {
    let running =
        sqlx::query_scalar!("SELECT COUNT(*) FROM process_instances WHERE state = 'RUNNING'")
            .fetch_one(&pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .unwrap_or(0);

    let waiting = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM process_instances WHERE state IN ('WAITING_FOR_TASK','WAITING_FOR_TIMER','WAITING_FOR_MESSAGE','WAITING_FOR_SIGNAL')"
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .unwrap_or(0);

    let completed =
        sqlx::query_scalar!("SELECT COUNT(*) FROM process_instances WHERE state = 'COMPLETED'")
            .fetch_one(&pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .unwrap_or(0);

    let failed =
        sqlx::query_scalar!("SELECT COUNT(*) FROM process_instances WHERE state = 'FAILED'")
            .fetch_one(&pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .unwrap_or(0);

    let pending_tasks = sqlx::query_scalar!("SELECT COUNT(*) FROM tasks WHERE state = 'CREATED'")
        .fetch_one(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .unwrap_or(0);

    let claimed_tasks = sqlx::query_scalar!("SELECT COUNT(*) FROM tasks WHERE state = 'CLAIMED'")
        .fetch_one(&pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .unwrap_or(0);

    Ok(Json(OverviewMetricsResponse {
        running_instances: running,
        waiting_instances: waiting,
        completed_instances: completed,
        failed_instances: failed,
        pending_tasks,
        claimed_tasks,
    }))
}
