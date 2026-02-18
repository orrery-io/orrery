use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
};
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::HashMap;

use orrery::diagram::parse_diagram_layout;
use orrery::parser::parse_bpmn;
use orrery_diagram::{render_svg, render_svg_with_counts};

use crate::errors::{codes, ApiError};

pub async fn get_diagram(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let inst_opt = sqlx::query!(
        "SELECT process_definition_id, process_definition_version, state, active_element_ids, variables FROM process_instances WHERE id = $1",
        id
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let inst = inst_opt.ok_or_else(|| {
        ApiError::not_found(
            codes::INSTANCE_NOT_FOUND,
            format!("Instance '{id}' not found"),
        )
    })?;

    let def = sqlx::query!(
        "SELECT bpmn_xml FROM process_definitions WHERE id = $1 AND version = $2",
        inst.process_definition_id,
        inst.process_definition_version
    )
    .fetch_one(&pool)
    .await
    .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let definition = parse_bpmn(&def.bpmn_xml).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Parse error: {e}"),
        )
    })?;

    let layout = parse_diagram_layout(&def.bpmn_xml);
    let mut active_ids: Vec<String> =
        serde_json::from_value(inst.active_element_ids).unwrap_or_default();

    // Gateways waiting for join tokens are not in active_element_ids, but the process
    // IS logically "at" that gateway. Extract gateway IDs from __join_counts__ so we
    // can highlight them as active in the diagram.
    {
        let vars: &serde_json::Value = &inst.variables;
        if let Some(jc) = vars
            .get("__join_counts__")
            .and_then(|v: &serde_json::Value| v.as_object())
        {
            for gw_id in jc.keys() {
                if !active_ids.contains(gw_id) {
                    active_ids.push(gw_id.clone());
                }
            }
        }
    }

    // When the instance is FAILED, the engine removes the token from the failed element
    // before setting state = Failed, so active_element_ids is empty. Fetch the failed
    // task's element_id so we can render it in red.
    let failed_ids: Vec<String> = if inst.state == "FAILED" {
        sqlx::query_scalar!(
            "SELECT element_id FROM tasks WHERE process_instance_id = $1 AND state = 'FAILED' \
             ORDER BY completed_at DESC LIMIT 1",
            id
        )
        .fetch_optional(&pool)
        .await
        .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .into_iter()
        .collect()
    } else {
        vec![]
    };

    let svg = render_svg(&definition, &layout, &active_ids, &failed_ids);

    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], svg))
}

/// GET /v1/process-definitions/:id/diagram
/// Returns an SVG showing aggregate active element counts across all RUNNING instances.
/// Optional `?version=N` query parameter scopes the BPMN and instance overlay to that version.
#[derive(Deserialize)]
pub struct DefinitionDiagramQuery {
    pub version: Option<i32>,
}

pub async fn get_definition_diagram(
    State(pool): State<PgPool>,
    Path(id): Path<String>,
    Query(q): Query<DefinitionDiagramQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let def = sqlx::query!(
        "SELECT bpmn_xml FROM process_definitions \
         WHERE id = $1 AND ($2::int IS NULL OR version = $2) \
         ORDER BY version DESC LIMIT 1",
        id,
        q.version,
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| {
        ApiError::not_found(
            codes::DEFINITION_NOT_FOUND,
            format!("Definition '{id}' not found"),
        )
    })?;

    let running = sqlx::query!(
        "SELECT active_element_ids FROM process_instances \
         WHERE process_definition_id = $1 \
           AND ($2::int IS NULL OR process_definition_version = $2) \
           AND state IN ('RUNNING', 'WAITING_FOR_TASK', 'WAITING_FOR_MESSAGE', 'WAITING_FOR_TIMER', 'WAITING_FOR_SIGNAL', 'FAILED')",
        id,
        q.version,
    )
    .fetch_all(&pool)
    .await
    .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // For FAILED instances the engine clears active_element_ids, so fall back to
    // the element_id recorded on the failed task itself.
    let failed_task_elements = sqlx::query_scalar!(
        "SELECT t.element_id FROM tasks t \
         JOIN process_instances pi ON t.process_instance_id = pi.id \
         WHERE pi.process_definition_id = $1 \
           AND ($2::int IS NULL OR pi.process_definition_version = $2) \
           AND pi.state = 'FAILED' AND t.state = 'FAILED'",
        id,
        q.version,
    )
    .fetch_all(&pool)
    .await
    .map_err(|e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut counts: HashMap<String, usize> = HashMap::new();
    for row in running {
        let ids: Vec<String> = serde_json::from_value(row.active_element_ids).unwrap_or_default();
        for eid in ids {
            *counts.entry(eid).or_insert(0) += 1;
        }
    }
    // Merge failed-task element counts (preserving duplicates for correct totals)
    // and build a set to track which elements have at least one failure.
    let mut failed_elements: std::collections::HashSet<String> = std::collections::HashSet::new();
    for eid in failed_task_elements {
        *counts.entry(eid.clone()).or_insert(0) += 1;
        failed_elements.insert(eid);
    }

    let definition = parse_bpmn(&def.bpmn_xml).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Parse error: {e}"),
        )
    })?;
    let layout = parse_diagram_layout(&def.bpmn_xml);
    let active_ids: Vec<String> = counts.keys().cloned().collect();
    let svg = render_svg_with_counts(&definition, &layout, &counts, &active_ids, &failed_elements);

    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], svg))
}
