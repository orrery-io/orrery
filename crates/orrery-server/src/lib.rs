pub mod db;
pub mod errors;
pub mod recovery;
pub mod routes;
pub mod scheduler;
pub mod timer_eval;

use axum::{
    body::Body,
    http::{header, Request},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    routing::post,
    routing::put,
    Json, Router,
};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_scalar::{Scalar, Servable};

/// Middleware that wraps non-2xx plain-text error responses in JSON `{"code": "...", "message": "..."}`.
/// Acts as a safety net for any responses not already using ApiError (e.g. framework-generated 405s).
async fn json_errors(req: Request<Body>, next: Next) -> Response {
    let resp = next.run(req).await;
    let status = resp.status();

    // Only transform error responses that are plain text
    if status.is_success() {
        return resp;
    }
    let is_text = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.starts_with("text/plain"))
        .unwrap_or(false);
    if !is_text {
        return resp;
    }

    // Read body and wrap in JSON with code + message
    let (parts, body) = resp.into_parts();
    let bytes = body
        .collect()
        .await
        .map(|c: http_body_util::Collected<_>| c.to_bytes())
        .unwrap_or_default();
    let message = String::from_utf8_lossy(&bytes).to_string();
    let json_body = serde_json::json!({
        "code": errors::codes::INTERNAL_ERROR,
        "message": message,
    });
    let mut new_resp = (parts.status, Json(json_body)).into_response();
    // Preserve original headers except content-type/content-length (set by Json)
    for (key, value) in parts.headers.iter() {
        if key != header::CONTENT_TYPE && key != header::CONTENT_LENGTH {
            new_resp.headers_mut().insert(key.clone(), value.clone());
        }
    }
    new_resp
}

use errors::ErrorResponse;
use routes::definitions::{
    DefinitionVersionsResponse, ListDefinitionsResponse, ProcessDefinitionResponse,
};
use routes::instances::{
    HistoryEntryResponse, PaginatedInstancesResponse, ProcessInstanceResponse,
    StartInstanceRequest, UpdateVariablesRequest,
};
use routes::messages::{SendMessageRequest, SendMessageResponse};
use routes::metrics::OverviewMetricsResponse;
use routes::signals::{BroadcastSignalRequest, BroadcastSignalResponse};
use routes::tasks::{
    ClaimRequest, CompleteTaskRequest as TaskCompleteRequest, FailRequest, TaskResponse,
};

#[derive(OpenApi)]
#[openapi(
    paths(
        routes::definitions::list,
        routes::definitions::deploy,
        routes::definitions::get,
        routes::definitions::list_versions,
        routes::instances::list,
        routes::instances::start,
        routes::instances::get,
        routes::instances::get_history,
        routes::instances::cancel,
        routes::instances::retry,
        routes::instances::update_variables,
        routes::tasks::list,
        routes::tasks::get_task,
        routes::tasks::claim,
        routes::tasks::complete,
        routes::tasks::fail,
        routes::tasks::retry,
        routes::messages::send_message,
        routes::signals::broadcast_signal,
        routes::metrics::overview,
    ),
    components(schemas(
        DefinitionVersionsResponse,
        ListDefinitionsResponse,
        ProcessDefinitionResponse,
        StartInstanceRequest,
        ProcessInstanceResponse,
        PaginatedInstancesResponse,
        HistoryEntryResponse,
        TaskResponse,
        ClaimRequest,
        TaskCompleteRequest,
        FailRequest,
        UpdateVariablesRequest,
        SendMessageRequest,
        SendMessageResponse,
        BroadcastSignalRequest,
        BroadcastSignalResponse,
        OverviewMetricsResponse,
        ErrorResponse,
    )),
    info(
        title = "Orrery API",
        version = "0.1.0",
        description = "Mechanical orchestration for your workflows",
    ),
    tags(
        (name = "Process Definitions", description = "Deploy and manage BPMN process definitions"),
        (name = "Process Instances", description = "Start and monitor process instances"),
        (name = "Tasks", description = "Claim and complete service tasks"),
        (name = "Events", description = "Send messages and signals to waiting process instances"),
        (name = "Metrics", description = "Aggregate counts for dashboard monitoring"),
    )
)]
pub struct ApiDoc;

pub fn build_app(pool: PgPool) -> Router {
    let mut router = Router::new()
        .route(
            "/v1/process-definitions",
            get(routes::definitions::list).post(routes::definitions::deploy),
        )
        .route(
            "/v1/process-definitions/{id}",
            get(routes::definitions::get),
        )
        .route(
            "/v1/process-definitions/{id}/versions",
            get(routes::definitions::list_versions),
        )
        .route(
            "/v1/process-instances",
            get(routes::instances::list).post(routes::instances::start),
        )
        .route("/v1/process-instances/{id}", get(routes::instances::get))
        .route(
            "/v1/process-instances/{id}/history",
            get(routes::instances::get_history),
        )
        .route(
            "/v1/process-instances/{id}/cancel",
            post(routes::instances::cancel),
        )
        .route(
            "/v1/process-instances/{id}/retry",
            post(routes::instances::retry),
        )
        .route(
            "/v1/process-instances/{id}/variables",
            put(routes::instances::update_variables),
        )
        .route(
            "/v1/process-instances/{id}/diagram",
            get(routes::diagram::get_diagram),
        )
        .route(
            "/v1/process-definitions/{id}/diagram",
            get(routes::diagram::get_definition_diagram),
        )
        .route("/v1/tasks", get(routes::tasks::list))
        .route("/v1/tasks/{id}", get(routes::tasks::get_task))
        .route("/v1/tasks/{id}/claim", post(routes::tasks::claim))
        .route("/v1/tasks/{id}/complete", post(routes::tasks::complete))
        .route("/v1/tasks/{id}/fail", post(routes::tasks::fail))
        .route("/v1/tasks/{id}/retry", post(routes::tasks::retry))
        .route("/v1/external-tasks", get(routes::external_tasks::list))
        .route(
            "/v1/external-tasks/fetch-and-lock",
            post(routes::external_tasks::fetch_and_lock),
        )
        .route(
            "/v1/external-tasks/{id}/complete",
            post(routes::external_tasks::complete),
        )
        .route(
            "/v1/external-tasks/{id}/failure",
            post(routes::external_tasks::failure),
        )
        .route(
            "/v1/external-tasks/{id}/extend-lock",
            post(routes::external_tasks::extend_lock),
        )
        .route("/v1/messages", post(routes::messages::send_message))
        .route(
            "/v1/signals/{name}",
            post(routes::signals::broadcast_signal),
        )
        .route(
            "/v1/process-instances/{id}/timers",
            get(routes::timers::list),
        )
        .route(
            "/v1/process-instances/{id}/timers/{timer_id}/fast-forward",
            post(routes::timers::fast_forward),
        )
        .route(
            "/v1/process-instances/{id}/timers/{timer_id}",
            put(routes::timers::update),
        )
        .route("/v1/metrics/overview", get(routes::metrics::overview))
        .route("/api-spec.json", get(|| async { Json(ApiDoc::openapi()) }))
        .merge(Scalar::with_url("/docs", ApiDoc::openapi()))
        .layer(middleware::from_fn(json_errors))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(pool);

    let ui_dir = std::env::var("UI_DIR").unwrap_or_else(|_| "./ui".to_string());
    if std::path::Path::new(&ui_dir).exists() {
        let serve = ServeDir::new(&ui_dir)
            .not_found_service(ServeFile::new(format!("{ui_dir}/index.html")));
        router = router.fallback_service(serve);
    }

    router
}
