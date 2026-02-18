use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;
use sqlx::PgPool;
use utoipa::ToSchema;

use orrery::model::{FlowElement, TimerKind};
use orrery::parser::parse_bpmn;

use crate::errors::{codes, ApiError, ErrorResponse};

#[derive(Serialize, ToSchema)]
pub struct ProcessDefinitionResponse {
    /// Process definition ID (taken from the BPMN `<process id="...">` attribute)
    pub id: String,
    /// Incremented each time the definition is re-deployed
    pub version: i32,
    /// UTC timestamp of when this version was created
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub running_count: i64,
    pub completed_count: i64,
    pub failed_count: i64,
}

#[utoipa::path(
    post,
    path = "/v1/process-definitions",
    request_body(
        content = String,
        description = "BPMN 2.0 XML document",
        content_type = "text/xml"
    ),
    responses(
        (status = 201, description = "Definition deployed", body = ProcessDefinitionResponse),
        (status = 422, description = "Invalid BPMN XML", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse),
    ),
    tag = "Process Definitions"
)]
pub async fn deploy(
    State(pool): State<PgPool>,
    body: String,
) -> Result<(StatusCode, Json<ProcessDefinitionResponse>), ApiError> {
    let definition = parse_bpmn(&body)
        .map_err(|e| ApiError::unprocessable(codes::INVALID_BPMN, format!("Invalid BPMN: {e}")))?;

    let id = definition.id.clone();

    let next_version: i32 = sqlx::query_scalar!(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM process_definitions WHERE id = $1",
        id,
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .unwrap_or(1);

    let row = sqlx::query!(
        r#"
        INSERT INTO process_definitions (id, version, bpmn_xml)
        VALUES ($1, $2, $3)
        RETURNING id, version, created_at
        "#,
        id,
        next_version,
        body,
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Register any timer start event definitions so the scheduler can fire them
    for elem in &definition.elements {
        if let FlowElement::TimerStartEvent(e) = elem {
            if let Some(ref timer_def) = e.timer {
                let kind_str = match timer_def.kind {
                    TimerKind::Duration => "duration",
                    TimerKind::Date => "date",
                    TimerKind::Cycle => "cycle",
                };
                let def_obj = orrery::model::TimerDefinition {
                    kind: timer_def.kind.clone(),
                    expression: timer_def.expression.clone(),
                };
                let first_due = crate::timer_eval::evaluate_due_at(&def_obj).ok();
                sqlx::query!(
                    "INSERT INTO timer_start_definitions \
                     (id, process_def_key, process_def_version, element_id, timer_kind, expression, next_due_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7) \
                     ON CONFLICT (process_def_key, process_def_version, element_id) DO UPDATE \
                         SET expression = EXCLUDED.expression, \
                             timer_kind = EXCLUDED.timer_kind, \
                             next_due_at = EXCLUDED.next_due_at, \
                             enabled = TRUE",
                    uuid::Uuid::new_v4().to_string(),
                    id,
                    next_version,
                    e.id,
                    kind_str,
                    timer_def.expression,
                    first_due,
                )
                .execute(&pool)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            }
        }
    }

    // Register any MessageStartEvent definitions so the message handler can start instances
    for elem in &definition.elements {
        if let FlowElement::MessageStartEvent(e) = elem {
            sqlx::query!(
                "INSERT INTO message_start_definitions
                     (id, process_def_key, process_def_version, element_id, message_name)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (process_def_key, process_def_version, element_id) DO UPDATE
                     SET message_name = EXCLUDED.message_name,
                         enabled = TRUE",
                uuid::Uuid::new_v4().to_string(),
                id,
                next_version,
                e.id,
                e.message_name,
            )
            .execute(&pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }
    }

    // Register any SignalStartEvent definitions so the signal handler can start instances
    for elem in &definition.elements {
        if let FlowElement::SignalStartEvent(e) = elem {
            sqlx::query(
                "INSERT INTO signal_start_definitions
                     (id, process_def_key, process_def_version, element_id, signal_ref)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (process_def_key, process_def_version, element_id) DO UPDATE
                     SET signal_ref = EXCLUDED.signal_ref,
                         enabled = TRUE",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&id)
            .bind(next_version)
            .bind(&e.id)
            .bind(&e.signal_ref)
            .execute(&pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }
    }

    // Disable MessageStartEvent definitions from previous versions of this process
    sqlx::query!(
        "UPDATE message_start_definitions SET enabled = FALSE
         WHERE process_def_key = $1 AND process_def_version < $2",
        id,
        next_version,
    )
    .execute(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Disable SignalStartEvent definitions from previous versions
    sqlx::query(
        "UPDATE signal_start_definitions SET enabled = FALSE
         WHERE process_def_key = $1 AND process_def_version < $2",
    )
    .bind(&id)
    .bind(next_version)
    .execute(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok((
        StatusCode::CREATED,
        Json(ProcessDefinitionResponse {
            id: row.id,
            version: row.version,
            created_at: row.created_at,
            running_count: 0,
            completed_count: 0,
            failed_count: 0,
        }),
    ))
}

#[derive(Serialize, ToSchema)]
pub struct ListDefinitionsResponse {
    pub items: Vec<ProcessDefinitionResponse>,
}

#[derive(Serialize, ToSchema)]
pub struct DefinitionVersionsResponse {
    pub versions: Vec<i32>,
    pub latest: i32,
}

#[utoipa::path(
    get,
    path = "/v1/process-definitions",
    responses(
        (status = 200, description = "List of process definitions", body = ListDefinitionsResponse),
        (status = 500, description = "Server error", body = ErrorResponse),
    ),
    tag = "Process Definitions"
)]
pub async fn list(State(pool): State<PgPool>) -> Result<Json<ListDefinitionsResponse>, ApiError> {
    let rows = sqlx::query!(
        r#"
        WITH latest AS (
            SELECT DISTINCT ON (id) id, version, created_at
            FROM process_definitions
            ORDER BY id, version DESC
        ),
        counts AS (
            SELECT
                process_definition_id,
                COUNT(*) FILTER (WHERE state IN ('RUNNING','WAITING_FOR_TASK','WAITING_FOR_TIMER',
                                                 'WAITING_FOR_MESSAGE','WAITING_FOR_SIGNAL')) AS running_count,
                COUNT(*) FILTER (WHERE state = 'COMPLETED') AS completed_count,
                COUNT(*) FILTER (WHERE state = 'FAILED') AS failed_count
            FROM process_instances
            GROUP BY process_definition_id
        )
        SELECT
            l.id,
            l.version,
            l.created_at,
            COALESCE(c.running_count, 0) AS "running_count!: i64",
            COALESCE(c.completed_count, 0) AS "completed_count!: i64",
            COALESCE(c.failed_count, 0) AS "failed_count!: i64"
        FROM latest l
        LEFT JOIN counts c ON c.process_definition_id = l.id
        ORDER BY l.created_at DESC
        "#
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ListDefinitionsResponse {
        items: rows
            .into_iter()
            .map(|r| ProcessDefinitionResponse {
                id: r.id,
                version: r.version,
                created_at: r.created_at,
                running_count: r.running_count,
                completed_count: r.completed_count,
                failed_count: r.failed_count,
            })
            .collect(),
    }))
}

#[utoipa::path(
    get,
    path = "/v1/process-definitions/{id}",
    params(
        ("id" = String, Path, description = "Process definition ID")
    ),
    responses(
        (status = 200, description = "Process definition found", body = ProcessDefinitionResponse),
        (status = 404, description = "Not found", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse),
    ),
    tag = "Process Definitions"
)]
pub async fn get(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
) -> Result<Json<ProcessDefinitionResponse>, ApiError> {
    let row = sqlx::query!(
        r#"
        SELECT id, version, created_at
        FROM process_definitions
        WHERE id = $1
        ORDER BY version DESC
        LIMIT 1
        "#,
        id
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| {
        ApiError::not_found(
            codes::DEFINITION_NOT_FOUND,
            format!("Process definition '{id}' not found"),
        )
    })?;

    Ok(Json(ProcessDefinitionResponse {
        id: row.id,
        version: row.version,
        created_at: row.created_at,
        running_count: 0,
        completed_count: 0,
        failed_count: 0,
    }))
}

#[utoipa::path(
    get,
    path = "/v1/process-definitions/{id}/versions",
    params(
        ("id" = String, Path, description = "Process definition ID")
    ),
    responses(
        (status = 200, description = "List of available versions", body = DefinitionVersionsResponse),
        (status = 404, description = "Not found", body = ErrorResponse),
        (status = 500, description = "Server error", body = ErrorResponse),
    ),
    tag = "Process Definitions"
)]
pub async fn list_versions(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
) -> Result<Json<DefinitionVersionsResponse>, ApiError> {
    let rows = sqlx::query_scalar!(
        "SELECT version FROM process_definitions WHERE id = $1 ORDER BY version DESC",
        id,
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if rows.is_empty() {
        return Err(ApiError::not_found(
            codes::DEFINITION_NOT_FOUND,
            format!("Process definition '{id}' not found"),
        ));
    }

    let latest = rows[0];
    Ok(Json(DefinitionVersionsResponse {
        versions: rows,
        latest,
    }))
}
