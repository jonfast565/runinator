use super::ApiDoc;
use utoipa::OpenApi;

#[test]
fn document_builds_and_serializes() {
    let doc = ApiDoc::openapi();
    let json = serde_json::to_value(&doc).expect("openapi serializes to json");

    assert_eq!(json["openapi"], "3.1.0");
    assert_eq!(json["info"]["title"], "Runinator Web Service API");
    assert!(
        !json["info"]["version"].as_str().unwrap_or("").is_empty(),
        "version is populated from the crate version"
    );
}

#[test]
fn annotated_paths_are_present() {
    let json = serde_json::to_value(ApiDoc::openapi()).unwrap();
    let paths = json["paths"].as_object().expect("paths object");

    for expected in [
        "/health",
        "/ready",
        "/auth/login",
        "/auth/refresh",
        "/auth/logout",
        "/auth/me",
        "/packs/import",
        "/workflows",
        "/workflows/import",
        "/gates/{id}/open",
        "/gates/{id}/close",
        "/workflow_runs/{id}/cancel",
        "/workflow_runs/{id}/pause",
        "/workflow_runs/{id}/resume",
        "/workflow_runs/{id}/replay",
        "/workflow_runs/{id}/rename",
    ] {
        assert!(
            paths.contains_key(expected),
            "missing documented path {expected}"
        );
    }
    // a public endpoint carries an explicit empty security requirement.
    assert_eq!(
        json["paths"]["/health"]["get"]["security"],
        serde_json::json!([])
    );
}

#[test]
fn security_schemes_are_registered() {
    let json = serde_json::to_value(ApiDoc::openapi()).unwrap();
    let schemes = &json["components"]["securitySchemes"];

    assert_eq!(schemes["bearerAuth"]["scheme"], "bearer");
    assert_eq!(schemes["bearerAuth"]["bearerFormat"], "JWT");
    assert_eq!(schemes["apiKeyAuth"]["in"], "header");
    assert_eq!(schemes["apiKeyAuth"]["name"], "X-Api-Key");
}

#[test]
fn auth_and_control_routes_expose_expected_schemas() {
    let json = serde_json::to_value(ApiDoc::openapi()).unwrap();

    assert_eq!(
        json["paths"]["/auth/refresh"]["post"]["security"],
        serde_json::json!([])
    );
    assert_eq!(
        json["paths"]["/auth/refresh"]["post"]["requestBody"]["content"]["application/json"]["schema"]
            ["$ref"],
        "#/components/schemas/RefreshRequestSchema"
    );
    assert_eq!(
        json["paths"]["/auth/logout"]["post"]["requestBody"]["content"]["application/json"]["schema"]
            ["$ref"],
        "#/components/schemas/RefreshRequestSchema"
    );
    assert_eq!(
        json["paths"]["/gates/{id}/open"]["post"]["requestBody"]["content"]["application/json"]["schema"]
            ["$ref"],
        "#/components/schemas/GateResolutionRequest"
    );
    assert_eq!(
        json["paths"]["/workflow_runs/{id}/rename"]["post"]["requestBody"]["content"]["application/json"]
            ["schema"]["$ref"],
        "#/components/schemas/WorkflowRunRenameRequest"
    );
    assert_eq!(
        json["paths"]["/workflow_runs/{id}/cancel"]["post"]["responses"]["200"]["content"]["application/json"]
            ["schema"]["$ref"],
        "#/components/schemas/TaskResponseSchema"
    );
}

#[test]
fn pack_import_docs_cover_zip_and_json_inputs() {
    let json = serde_json::to_value(ApiDoc::openapi()).unwrap();
    let post = &json["paths"]["/packs/import"]["post"];

    assert!(post["requestBody"]["content"]["application/zip"].is_object());
    assert!(post["requestBody"]["content"]["application/json"].is_object());
    assert_eq!(
        post["parameters"][0]["name"], "overwrite",
        "pack import keeps the overwrite query parameter documented"
    );
    assert_eq!(
        post["parameters"][1]["name"],
        "x-runinator-json-workflow-risk"
    );
}

#[test]
fn scalar_docs_point_at_generated_openapi_json() {
    assert!(super::SCALAR_HTML.contains("data-url=\"/openapi.json\""));
}
