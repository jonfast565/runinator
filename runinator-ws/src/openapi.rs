//! openapi reference scaffold. the document is generated automatically by `utoipa` from the
//! `#[utoipa::path]` annotations on the handlers registered in [`ApiDoc`]; it is served as raw json at
//! `/openapi.json` and as an interactive reference at `/docs`.
//!
//! to document a new endpoint: add a `#[utoipa::path(...)]` attribute to its handler (mirroring the
//! route's method/path), then list the handler in the `paths(...)` set below. derive `ToSchema` on any
//! request/response struct referenced by `body = ...` so its schema is emitted too.

use axum::Json;
use axum::response::Html;
use utoipa::openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

use crate::models::ApiError;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Runinator Web Service API",
        description = "HTTP API for the Runinator orchestrator: workflows, runs, triggers, \
                       providers, credentials, and the scheduler/executor control plane. Most \
                       endpoints return the shared `ApiResponse` envelope (an untagged union); \
                       schemas are filled in incrementally as handlers gain `#[utoipa::path]` \
                       annotations.",
    ),
    modifiers(&SecurityAddon),
    security(("bearerAuth" = []), ("apiKeyAuth" = [])),
    tags(
        (name = "Meta", description = "Health, readiness, and the api reference."),
        (name = "Auth", description = "Login, tokens, and the current principal."),
        (name = "Workflows", description = "Workflow definitions."),
        (name = "Workflow Runs", description = "Workflow run lifecycle."),
        (name = "Providers", description = "Registered task providers."),
        (name = "Replicas", description = "Service replica registry."),
    ),
    paths(
        crate::handlers::health::health,
        crate::handlers::health::ready,
        crate::handlers::auth::auth_config,
        crate::handlers::auth::login,
        crate::handlers::auth::me,
        crate::handlers::workflows::get_workflows,
        crate::handlers::runs::get_workflow_runs,
        crate::handlers::providers::get_providers,
        crate::handlers::replicas::get_replicas,
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

/// the generated openapi document as json.
pub(crate) async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

/// an interactive api reference (Scalar) that loads `/openapi.json`.
pub(crate) async fn openapi_docs() -> Html<&'static str> {
    Html(SCALAR_HTML)
}

#[cfg(test)]
#[path = "openapi_tests.rs"]
mod tests;

const SCALAR_HTML: &str = r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Runinator API Reference</title>
  </head>
  <body>
    <script id="api-reference" data-url="/openapi.json"></script>
    <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
  </body>
</html>
"#;
