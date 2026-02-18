use utoipa::OpenApi;

#[test]
fn api_spec_has_all_paths() {
    let spec = orrery_server::ApiDoc::openapi();
    let json = serde_json::to_value(&spec).unwrap();
    let paths = json["paths"].as_object().unwrap();

    assert!(
        paths.contains_key("/v1/process-definitions"),
        "missing POST /v1/process-definitions"
    );
    assert!(
        paths.contains_key("/v1/process-definitions/{id}"),
        "missing GET /v1/process-definitions/{{id}}"
    );
    assert!(
        paths.contains_key("/v1/process-instances"),
        "missing POST /v1/process-instances"
    );
    assert!(
        paths.contains_key("/v1/process-instances/{id}"),
        "missing GET /v1/process-instances/{{id}}"
    );
    assert!(
        paths.contains_key("/v1/process-instances/{id}/history"),
        "missing GET history"
    );
    assert!(paths.contains_key("/v1/tasks"), "missing GET /v1/tasks");
    assert!(
        paths.contains_key("/v1/tasks/{id}"),
        "missing GET /v1/tasks/{{id}}"
    );
    assert!(
        paths.contains_key("/v1/tasks/{id}/claim"),
        "missing POST /v1/tasks/{{id}}/claim"
    );
    assert!(
        paths.contains_key("/v1/tasks/{id}/complete"),
        "missing POST /v1/tasks/{{id}}/complete"
    );
    assert!(
        paths.contains_key("/v1/tasks/{id}/fail"),
        "missing POST /v1/tasks/{{id}}/fail"
    );
}
