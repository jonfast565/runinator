//! openapi reference scaffold. the document is generated automatically by `utoipa` from the
//! `#[utoipa::path]` annotations on the handlers registered in [`ApiDoc`]; it is served as raw json at
//! `/openapi.json` and as an interactive reference at `/docs`.
//!
//! to document a new endpoint: add a `#[utoipa::path(...)]` attribute to its handler (mirroring the
//! route's method/path), then list the handler in the `paths(...)` set below. derive `ToSchema` on any
//! request/response struct referenced by `body = ...` so its schema is emitted too.

use axum::Json;
use axum::response::Html;
use serde_json::{Map, Value, json};
use utoipa::openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

use crate::handlers;
use crate::models::ApiError;
use crate::websocket;

// the doc vocabulary is shared with every handler crate, so it lives in runinator-ws-core; these
// re-exports keep the `crate::openapi::docs` path the handlers and the WebSocket module already use.
pub(crate) use runinator_ws_core::openapi::{docs, examples};

use docs::{EndpointDoc, Example, RequestDoc, endpoint};
use examples::{UUID_EXAMPLE, example_value};

/// every module's endpoint documentation, in the order the tags should read.
///
/// each entry is the `DOCS` slice sitting beside that module's `routes()` fn, so an endpoint's
/// registration and its documentation are added in one place. `route_parity` diffs the union of
/// these against the union of the registered routes.
const DOC_SETS: &[&[EndpointDoc]] = &[
    handlers::health::DOCS,
    DOCS,
    websocket::DOCS,
    handlers::workflows::DOCS,
    handlers::rexrap::DOCS,
    handlers::packs::DOCS,
    handlers::triggers::DOCS,
    handlers::runs::DOCS,
    handlers::replicas::DOCS,
    handlers::agents::DOCS,
    handlers::artifacts::DOCS,
    handlers::notifications::DOCS,
    handlers::action_dispatches::DOCS,
    handlers::debug::DOCS,
    handlers::supervisor::DOCS,
    handlers::node_runs::DOCS,
    handlers::catalog::DOCS,
    handlers::automation::DOCS,
    handlers::credentials::DOCS,
    handlers::providers::DOCS,
    handlers::functions::DOCS,
    handlers::function_invocations::DOCS,
    handlers::console::DOCS,
    handlers::webhook::DOCS,
    handlers::auth::DOCS,
    handlers::authz::DOCS,
];

/// Routes whose full prose/examples have not yet been authored still declare an explicit policy
/// and receive a minimal OpenAPI operation. This keeps route, documentation, and authorization
/// parity fail-closed without making descriptive documentation a prerequisite for enforcement.
pub(crate) const MINIMAL_ENDPOINTS: &[(&str, &str, docs::EndpointPolicy)] = &[
    (
        "get",
        "/audit_log",
        docs::EndpointPolicy::ScopedAction(runinator_models::rbac::Action::AuditRead),
    ),
    (
        "get",
        "/dead_letters",
        docs::EndpointPolicy::ScopedAction(runinator_models::rbac::Action::DeadLettersRead),
    ),
    ("get", "/catalog/enums", docs::EndpointPolicy::Authenticated),
    ("get", "/metrics", docs::EndpointPolicy::Public),
    ("get", "/node-kinds", docs::EndpointPolicy::Authenticated),
    ("get", "/trigger-kinds", docs::EndpointPolicy::Authenticated),
    (
        "get",
        "/nodes",
        docs::EndpointPolicy::SystemRole(&[
            runinator_models::rbac::SystemRole::Engine,
            runinator_models::rbac::SystemRole::Replica,
        ]),
    ),
    (
        "get",
        "/nodes/backends",
        docs::EndpointPolicy::SystemRole(&[
            runinator_models::rbac::SystemRole::Engine,
            runinator_models::rbac::SystemRole::Replica,
        ]),
    ),
    (
        "get",
        "/workflows/{id}/revisions",
        docs::EndpointPolicy::ResourceAction(
            runinator_models::auth::ResourceType::Workflow,
            runinator_models::rbac::Action::View,
        ),
    ),
    (
        "get",
        "/workflows/{id}/revisions/{revision}",
        docs::EndpointPolicy::ResourceAction(
            runinator_models::auth::ResourceType::Workflow,
            runinator_models::rbac::Action::View,
        ),
    ),
    (
        "post",
        "/workflows/{id}/revisions/{revision}/restore",
        docs::EndpointPolicy::ResourceAction(
            runinator_models::auth::ResourceType::Workflow,
            runinator_models::rbac::Action::Edit,
        ),
    ),
    (
        "get",
        "/ws/desktop-worker",
        docs::EndpointPolicy::SystemRole(&[
            runinator_models::rbac::SystemRole::Agent,
            runinator_models::rbac::SystemRole::Worker,
        ]),
    ),
    (
        "delete",
        "/artifacts/{id}",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "delete",
        "/automation_events/{id}",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "delete",
        "/freeze_windows/{id}",
        docs::EndpointPolicy::Authenticated,
    ),
    ("delete", "/gates/{id}", docs::EndpointPolicy::Authenticated),
    (
        "delete",
        "/notification_policies/{id}",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "delete",
        "/notifications/{id}",
        docs::EndpointPolicy::Authenticated,
    ),
    ("delete", "/orgs/{id}", docs::EndpointPolicy::Authenticated),
    (
        "delete",
        "/orgs/{id}/members/{user_id}",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "delete",
        "/pipeline_triggers/{id}",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "delete",
        "/pipelines/{id}",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "get",
        "/freeze_windows",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "get",
        "/notification_policies",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "get",
        "/notifications/{id}/deliveries",
        docs::EndpointPolicy::Authenticated,
    ),
    ("get", "/orgs", docs::EndpointPolicy::Authenticated),
    ("get", "/orgs/me", docs::EndpointPolicy::Authenticated),
    ("get", "/orgs/{id}", docs::EndpointPolicy::Authenticated),
    (
        "get",
        "/orgs/{id}/members",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "get",
        "/orgs/{id}/nodes",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "get",
        "/orgs/{id}/quota",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "get",
        "/orgs/{id}/usage",
        docs::EndpointPolicy::Authenticated,
    ),
    ("get", "/pipeline_runs", docs::EndpointPolicy::Authenticated),
    (
        "get",
        "/pipeline_runs/{id}",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "delete",
        "/pipeline_runs/{id}",
        docs::EndpointPolicy::Authenticated,
    ),
    ("get", "/pipelines", docs::EndpointPolicy::Authenticated),
    (
        "get",
        "/pipelines/{id}",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "get",
        "/pipelines/{id}/triggers",
        docs::EndpointPolicy::Authenticated,
    ),
    ("get", "/rate-card", docs::EndpointPolicy::Authenticated),
    (
        "get",
        "/replicas/{replica_id}/samples",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "get",
        "/workflow_runs/{id}/transitions",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "delete",
        "/workflow_runs/{id}",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "get",
        "/workflows/{id}/nodes/{node_id}/transitions",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "patch",
        "/freeze_windows/{id}",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "patch",
        "/notification_policies/{id}",
        docs::EndpointPolicy::Authenticated,
    ),
    ("patch", "/orgs/{id}", docs::EndpointPolicy::Authenticated),
    (
        "patch",
        "/orgs/{id}/members/{user_id}",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "patch",
        "/pipeline_triggers/{id}",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "patch",
        "/pipelines/{id}",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "post",
        "/auth/switch-org",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "post",
        "/freeze_windows",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "post",
        "/idempotency_keys/claim",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "post",
        "/idempotency_keys/complete",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "post",
        "/idempotency_keys/release",
        docs::EndpointPolicy::Authenticated,
    ),
    ("post", "/nodes/scale", docs::EndpointPolicy::Authenticated),
    ("post", "/nodes/stop", docs::EndpointPolicy::Authenticated),
    (
        "post",
        "/notification_policies",
        docs::EndpointPolicy::Authenticated,
    ),
    ("post", "/orgs", docs::EndpointPolicy::Authenticated),
    (
        "post",
        "/orgs/{id}/members",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "post",
        "/orgs/{id}/nodes/scale",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "post",
        "/pipeline_runs/{id}/cancel",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "post",
        "/pipeline_runs/{id}/pause",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "post",
        "/pipeline_runs/{id}/resume",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "post",
        "/pipeline_runs/{id}/resolve",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "post",
        "/pipeline_runs/{id}/members/{member_key}/retry",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "post",
        "/pipeline_triggers/{id}/runs",
        docs::EndpointPolicy::Authenticated,
    ),
    ("post", "/pipelines", docs::EndpointPolicy::Authenticated),
    (
        "post",
        "/pipelines/{id}/runs",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "post",
        "/pipelines/{id}/triggers",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "post",
        "/workflow_triggers/{id}/backfill",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "post",
        "/workflows/simulate",
        docs::EndpointPolicy::Authenticated,
    ),
    (
        "put",
        "/orgs/{id}/quota",
        docs::EndpointPolicy::Authenticated,
    ),
];

/// the documented endpoints, flattened across [`DOC_SETS`].
fn endpoint_docs() -> impl Iterator<Item = &'static EndpointDoc> {
    DOC_SETS.iter().copied().flatten()
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Runinator Web Service API",
        description = "HTTP API for the Runinator orchestrator. The public surface manages \
                       workflows, REXRAP authoring, workflow runs, triggers, providers, credentials, \
                       automation records, auth, replicas, and runtime control-plane operations. \
                       The OpenAPI document is enriched after utoipa generation so every route has \
                       route text and request examples even when a handler does not yet expose a \
                       typed schema.",
    ),
    modifiers(&SecurityAddon),
    security(("bearerAuth" = []), ("apiKeyAuth" = [])),
    tags(
        (name = "Meta", description = "Health, readiness, and the api reference."),
        (name = "Auth", description = "Login, tokens, and the current principal."),
        (name = "Packs", description = "Workflow and compiled pack import flows."),
        (name = "Workflows", description = "Workflow definitions."),
        (name = "Workflow Runs", description = "Workflow run lifecycle."),
        (name = "Automation", description = "Automation records such as gates and approvals."),
        (name = "Artifacts", description = "Run and node-run artifacts."),
        (name = "Catalog", description = "Catalog entries used by authoring and provider metadata."),
        (name = "Control Plane", description = "Scheduler, worker, and service-to-service endpoints."),
        (name = "Credentials", description = "Secret and config settings."),
        (name = "Debug", description = "Workflow-run debugger commands."),
        (name = "Notifications", description = "User notification records."),
        (name = "Providers", description = "Registered task providers."),
        (name = "Functions", description = "Packaged functions: publishing, promotion, and artifacts."),
        (name = "Console", description = "The REXRAP console: notebook sessions and their cells."),
        (name = "Replicas", description = "Service replica registry."),
        (name = "Runs", description = "Low-level task run records."),
        (name = "Supervisor", description = "Local supervisor status."),
        (name = "Webhooks", description = "External webhook ingress."),
        (name = "REXRAP", description = "REXRAP language tooling."),
        (name = "WebSockets", description = "Streaming API endpoints."),
    ),
    paths(
        crate::handlers::health::health,
        crate::handlers::health::metrics,
        crate::handlers::health::ready,
        crate::handlers::auth::auth_config,
        crate::handlers::auth::login,
        crate::handlers::auth::refresh,
        crate::handlers::auth::logout,
        crate::handlers::auth::me,
        crate::handlers::packs::import_pack,
        crate::handlers::workflows::get_workflows,
        crate::handlers::workflows::get_workflow_revisions,
        crate::handlers::workflows::get_workflow_revision,
        crate::handlers::workflows::restore_workflow_revision,
        crate::handlers::workflows::import_workflow_bundle,
        crate::handlers::automation::open_gate,
        crate::handlers::automation::close_gate,
        crate::handlers::runs::cancel_workflow_run,
        crate::handlers::runs::pause_workflow_run,
        crate::handlers::runs::resume_workflow_run,
        crate::handlers::runs::replay_workflow_run,
        crate::handlers::runs::rename_workflow_run,
        crate::handlers::runs::get_workflow_runs,
        crate::handlers::providers::get_providers,
        crate::handlers::catalog_metadata::get_node_kinds,
        crate::handlers::catalog_metadata::get_trigger_kinds,
        crate::handlers::catalog_metadata::get_enum_catalogs,
        crate::handlers::replicas::get_replicas,
        crate::handlers::provisioning::get_node_backends,
        crate::handlers::provisioning::get_nodes,
        crate::handlers::observability::get_dead_letters,
        crate::handlers::observability::get_audit_log,
    ),
    components(schemas(ApiError)),
)]
pub struct ApiDoc;

/// inject the two accepted credentials: a bearer JWT (from `/auth/login`) or an `X-Api-Key`.
struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "bearerAuth",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );
        components.add_security_scheme(
            "apiKeyAuth",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("X-Api-Key"))),
        );
    }
}

/// the generated and route-enriched openapi document as json.
pub(crate) async fn openapi_json() -> Json<Value> {
    Json(openapi_document())
}

pub(crate) fn openapi_document() -> Value {
    let mut document = serde_json::to_value(ApiDoc::openapi())
        .expect("generated openapi document serializes to json");
    enrich_openapi_document(&mut document);
    document
}

/// an interactive api reference (Scalar) that loads `/openapi.json`.
pub(crate) async fn openapi_docs() -> Html<&'static str> {
    Html(SCALAR_HTML)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod route_parity;

const SCALAR_HTML: &str = r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Runinator API Reference</title>
  </head>
  <body>
    <script
      id="api-reference"
      data-url="/openapi.json"
      data-configuration='{"layout":"modern","defaultHttpClient":{"targetKey":"shell","clientKey":"curl"}}'
    ></script>
    <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
  </body>
</html>
"#;

fn enrich_openapi_document(document: &mut Value) {
    let Some(paths) = document.get_mut("paths").and_then(Value::as_object_mut) else {
        return;
    };

    for doc in endpoint_docs() {
        let path_item = paths
            .entry(doc.path.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        let path_item = path_item.as_object_mut().expect("path item is an object");
        let operation = path_item
            .entry(doc.method.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        enrich_operation(operation, doc);
    }
    for (method, path, policy) in MINIMAL_ENDPOINTS {
        let path_item = paths
            .entry((*path).to_string())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .expect("path item is an object");
        let operation = path_item
            .entry((*method).to_string())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .expect("operation is an object");
        operation
            .entry("summary")
            .or_insert_with(|| json!(format!("{} {path}", method.to_uppercase())));
        operation.entry("description").or_insert_with(|| {
            json!("Authorization is enforced by the registered handler policy.")
        });
        operation.insert(
            "x-runinator-authorization".into(),
            endpoint_policy_json(*policy),
        );
        operation
            .entry("responses")
            .or_insert_with(|| json!({ "200": { "description": "successful response" } }));
    }
}

fn enrich_operation(operation: &mut Value, doc: &EndpointDoc) {
    let operation = operation.as_object_mut().expect("operation is an object");
    operation.insert("tags".into(), json!([doc.tag]));
    operation.insert("summary".into(), json!(doc.summary));
    operation.insert("description".into(), json!(doc.description));
    if doc.policy.is_public() {
        operation.insert("security".into(), json!([]));
    }
    operation.insert(
        "x-runinator-authorization".into(),
        endpoint_policy_json(doc.policy),
    );
    operation.insert("parameters".into(), json!(parameters_for(doc)));
    if let Some(request) = doc.request {
        enrich_request_body(operation, request);
    }
    enrich_success_response(operation, doc);
    operation.insert(
        "x-codeSamples".into(),
        json!([{
            "lang": "Shell",
            "label": "curl",
            "source": curl_sample(doc),
        }]),
    );
}

fn parameters_for(doc: &EndpointDoc) -> Vec<Value> {
    let mut params = Vec::new();
    for name in path_parameters(doc.path) {
        params.push(json!({
            "name": name,
            "in": "path",
            "required": true,
            "description": format!("{} identifier from the route.", name.replace('_', " ")),
            "schema": { "type": "string", "format": "uuid" },
            "example": UUID_EXAMPLE,
        }));
    }
    for param in doc.query {
        params.push(json!({
            "name": param.name,
            "in": param.location,
            "required": param.required,
            "description": param.description,
            "schema": { "type": "string" },
            "example": param.example,
        }));
    }
    params
}

fn path_parameters(path: &str) -> Vec<&str> {
    path.split('/')
        .filter_map(|segment| {
            segment
                .strip_prefix('{')
                .and_then(|segment| segment.strip_suffix('}'))
        })
        .collect()
}

fn enrich_request_body(operation: &mut Map<String, Value>, request: RequestDoc) {
    let request_body = operation
        .entry("requestBody")
        .or_insert_with(|| json!({ "content": {} }));
    let request_body = request_body
        .as_object_mut()
        .expect("request body is an object");
    request_body.insert("description".into(), json!(request.description));
    request_body.entry("required").or_insert(json!(true));
    let content = request_body
        .entry("content")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .expect("request content is an object");
    if content.is_empty() {
        content.insert(
            request.content_type.into(),
            json!({ "schema": { "type": "object" } }),
        );
    }
    for (content_type, media) in content.iter_mut() {
        if content_type != request.content_type && request.content_type != "application/zip" {
            continue;
        }
        let media = media.as_object_mut().expect("media type is an object");
        media
            .entry("schema")
            .or_insert_with(|| json!({ "type": "object" }));
        if let Some(example) = example_value(request.example) {
            media.insert("example".into(), example);
        }
    }
}

fn enrich_success_response(operation: &mut Map<String, Value>, doc: &EndpointDoc) {
    let responses = operation
        .entry("responses")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .expect("responses is an object");
    let status = doc.success_status.to_string();
    let response = responses
        .entry(status)
        .or_insert_with(|| json!({ "description": doc.success_description }));
    let response = response.as_object_mut().expect("response is an object");
    response.insert("description".into(), json!(doc.success_description));
    if matches!(doc.success_status, 101) {
        return;
    }
    let Some(example) = example_value(doc.response_example) else {
        return;
    };
    let content = response
        .entry("content")
        .or_insert_with(|| json!({ "application/json": { "schema": { "type": "object" } } }))
        .as_object_mut()
        .expect("response content is an object");
    let media = content
        .entry("application/json")
        .or_insert_with(|| json!({ "schema": { "type": "object" } }))
        .as_object_mut()
        .expect("json response media is an object");
    media
        .entry("schema")
        .or_insert_with(|| json!({ "type": "object" }));
    media.insert("example".into(), example);
}

fn curl_sample(doc: &EndpointDoc) -> String {
    let mut path = doc.path.to_string();
    for name in path_parameters(doc.path) {
        path = path.replace(&format!("{{{name}}}"), UUID_EXAMPLE);
    }
    let mut command = format!(
        "curl -X {} http://127.0.0.1:8080{}",
        doc.method.to_uppercase(),
        path
    );
    if !doc.policy.is_public() {
        command.push_str(" \\\n  -H 'Authorization: Bearer $RUNINATOR_TOKEN'");
    }
    if let Some(request) = doc.request {
        command.push_str(&format!(
            " \\\n  -H 'Content-Type: {}'",
            request.content_type
        ));
        if let Some(example) = example_value(request.example) {
            command.push_str(&format!(" \\\n  --data '{}'", compact_json(example)));
        }
    }
    for param in doc.query {
        if param.location == "header" {
            command.push_str(&format!(" \\\n  -H '{}: {}'", param.name, param.example));
        }
    }
    command
}

fn endpoint_policy_json(policy: docs::EndpointPolicy) -> Value {
    use docs::EndpointPolicy;
    match policy {
        EndpointPolicy::Public => json!({ "kind": "public" }),
        EndpointPolicy::Authenticated => json!({ "kind": "authenticated" }),
        EndpointPolicy::ScopedAction(action) => {
            json!({ "kind": "scoped_action", "action": action.as_str() })
        }
        EndpointPolicy::AnyResourceAction(action) => {
            json!({ "kind": "resource_action", "resource_type": "path", "action": action.as_str() })
        }
        EndpointPolicy::ResourceAction(resource, action) => json!({
            "kind": "resource_action",
            "resource_type": resource.as_str(),
            "action": action.as_str(),
        }),
        EndpointPolicy::SystemRole(roles) => json!({
            "kind": "system_role",
            "roles": roles.iter().map(|role| role.as_str()).collect::<Vec<_>>(),
        }),
    }
}

fn compact_json(value: Value) -> String {
    serde_json::to_string(&value).expect("example serializes")
}

/// the self-describing api surface: the raw document and the reference ui.
pub(crate) fn routes() -> axum::Router {
    use axum::routing::get;
    axum::Router::new()
        .route("/openapi.json", get(crate::openapi::openapi_json))
        .route("/docs", get(crate::openapi::openapi_docs))
}

/// the openapi entries for the routes above.
pub(crate) const DOCS: &[EndpointDoc] = &[
    endpoint(
        "get",
        "/openapi.json",
        "Meta",
        "Download the OpenAPI document",
        "Returns the same enriched OpenAPI 3.1 document used by the Scalar reference.",
        true,
        None,
        &[],
        200,
        "openapi document",
        Example::None,
    ),
    endpoint(
        "get",
        "/docs",
        "Meta",
        "Open the Scalar API reference",
        "Serves the browser UI for exploring this OpenAPI document.",
        true,
        None,
        &[],
        200,
        "html api reference",
        Example::None,
    ),
];

#[cfg(test)]
mod policy_tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn only_the_explicit_bootstrap_surface_is_public() {
        let actual = endpoint_docs()
            .filter(|doc| doc.policy.is_public())
            .map(|doc| (doc.method, doc.path))
            .chain(
                MINIMAL_ENDPOINTS
                    .iter()
                    .filter(|(_, _, policy)| policy.is_public())
                    .map(|(method, path, _)| (*method, *path)),
            )
            .collect::<BTreeSet<_>>();
        let expected = BTreeSet::from([
            ("get", "/auth/config"),
            ("post", "/auth/login"),
            ("post", "/auth/refresh"),
            ("post", "/agents/enroll"),
            ("get", "/health"),
            ("get", "/ready"),
            ("get", "/metrics"),
            ("get", "/openapi.json"),
            ("get", "/docs"),
        ]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn every_openapi_operation_exports_its_authorization_policy() {
        let document = openapi_document();
        let mut missing = Vec::new();
        for (path, item) in document["paths"].as_object().expect("paths object") {
            for method in ["get", "post", "put", "patch", "delete"] {
                let Some(operation) = item.get(method) else {
                    continue;
                };
                if operation.get("x-runinator-authorization").is_none() {
                    missing.push(format!("{method} {path}"));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "missing authorization metadata: {missing:?}"
        );
    }
}
