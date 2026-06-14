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

    for expected in ["/health", "/ready", "/auth/login", "/auth/me", "/workflows"] {
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
