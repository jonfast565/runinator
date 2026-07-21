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

use crate::models::ApiError;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Runinator Web Service API",
        description = "HTTP API for the Runinator orchestrator. The public surface manages \
                       workflows, WDL authoring, workflow runs, triggers, providers, credentials, \
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
        (name = "Replicas", description = "Service replica registry."),
        (name = "Runs", description = "Low-level task run records."),
        (name = "Supervisor", description = "Local supervisor status."),
        (name = "Webhooks", description = "External webhook ingress."),
        (name = "WDL", description = "WDL language tooling."),
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
    <script
      id="api-reference"
      data-url="/openapi.json"
      data-configuration='{"layout":"modern","defaultHttpClient":{"targetKey":"shell","clientKey":"curl"}}'
    ></script>
    <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
  </body>
</html>
"#;

#[derive(Clone, Copy)]
struct EndpointDoc {
    method: &'static str,
    path: &'static str,
    tag: &'static str,
    summary: &'static str,
    description: &'static str,
    public: bool,
    request: Option<RequestDoc>,
    query: &'static [ParamDoc],
    success_status: u16,
    success_description: &'static str,
    response_example: Example,
}

#[derive(Clone, Copy)]
struct RequestDoc {
    description: &'static str,
    example: Example,
    content_type: &'static str,
}

#[derive(Clone, Copy)]
struct ParamDoc {
    name: &'static str,
    location: &'static str,
    description: &'static str,
    required: bool,
    example: &'static str,
}

#[derive(Clone, Copy)]
enum Example {
    None,
    Health,
    Ready,
    AuthConfig,
    LoginRequest,
    RefreshRequest,
    LoginResponse,
    TaskResponse,
    Workflow,
    WorkflowList,
    WorkflowBundle,
    WorkflowRunRequest,
    WorkflowRun,
    WorkflowRunList,
    WorkflowRunStatus,
    WorkflowRunReplay,
    WorkflowRunRename,
    RunList,
    RunStatus,
    RunChunk,
    Artifact,
    WdlSource,
    WdlCompile,
    WdlCompletion,
    WdlHover,
    WdlDiagnostics,
    WdlDecompile,
    WdlEvaluate,
    Trigger,
    TriggerList,
    TriggerClaim,
    SchedulerRunClaim,
    SchedulerReadyNodeClaim,
    SchedulerRunLease,
    ActionDispatch,
    ActionDispatchList,
    ReadyNodeProcess,
    NodeRun,
    NodeRunStatus,
    NodeRunInput,
    NodeRunClaim,
    NodeRunRelease,
    ArtifactList,
    CatalogItem,
    AutomationRecord,
    GateResolution,
    ApprovalResolution,
    Idempotency,
    Credential,
    SecretBundle,
    Provider,
    ProviderList,
    ProviderBundle,
    Replica,
    ReplicaList,
    ReplicaProvider,
    Notification,
    NotificationList,
    User,
    UserList,
    ApiKey,
    ApiKeyList,
    Grant,
    Team,
    WebhookWake,
    WebhookSignal,
    Supervisor,
}

const UUID_EXAMPLE: &str = "018f5f7c-4b74-7f44-8fd1-cde6b5c4d111";
const CURSOR: &[ParamDoc] = &[
    ParamDoc {
        name: "cursor",
        location: "query",
        description: "Return chunks after this numeric cursor.",
        required: false,
        example: "0",
    },
    ParamDoc {
        name: "limit",
        location: "query",
        description: "Maximum number of chunks to return.",
        required: false,
        example: "100",
    },
];
const WORKFLOW_FILTERS: &[ParamDoc] = &[ParamDoc {
    name: "name",
    location: "query",
    description: "Exact workflow name to fetch.",
    required: false,
    example: "hello-world",
}];
const WORKFLOW_RUN_FILTERS: &[ParamDoc] = &[
    ParamDoc {
        name: "status",
        location: "query",
        description: "Filter runs by workflow status.",
        required: false,
        example: "running",
    },
    ParamDoc {
        name: "workflow_id",
        location: "query",
        description: "Filter runs for one workflow definition.",
        required: false,
        example: UUID_EXAMPLE,
    },
    ParamDoc {
        name: "name",
        location: "query",
        description: "Filter runs by display name.",
        required: false,
        example: "nightly deploy",
    },
    ParamDoc {
        name: "open",
        location: "query",
        description: "When filtering by name, only return open runs.",
        required: false,
        example: "true",
    },
];
const RUN_FILTERS: &[ParamDoc] = &[ParamDoc {
    name: "status",
    location: "query",
    description: "Required low-level task run status filter.",
    required: true,
    example: "running",
}];
const PACK_IMPORT_PARAMS: &[ParamDoc] = &[
    ParamDoc {
        name: "overwrite",
        location: "query",
        description: "Replace existing workflows and settings from the pack when true.",
        required: false,
        example: "true",
    },
    ParamDoc {
        name: "x-runinator-json-workflow-risk",
        location: "header",
        description: "Required only when posting raw JSON to the pack import endpoint.",
        required: false,
        example: "system-breakage-possible",
    },
];
const WORKFLOW_IMPORT_HEADERS: &[ParamDoc] = &[ParamDoc {
    name: "x-runinator-json-workflow-risk",
    location: "header",
    description: "Required acknowledgement for importing raw JSON workflow bundles.",
    required: true,
    example: "system-breakage-possible",
}];
const WORKFLOW_TRIGGER_FILTERS: &[ParamDoc] = &[ParamDoc {
    name: "status",
    location: "query",
    description: "Filter due triggers by status when supported by the caller.",
    required: false,
    example: "enabled",
}];
const REPLICA_FILTERS: &[ParamDoc] = &[
    ParamDoc {
        name: "replica_type",
        location: "query",
        description: "Filter replicas by kind.",
        required: false,
        example: "worker",
    },
    ParamDoc {
        name: "status",
        location: "query",
        description: "Filter replicas by current status.",
        required: false,
        example: "online",
    },
];
const CATALOG_FILTERS: &[ParamDoc] = &[
    ParamDoc {
        name: "item_type",
        location: "query",
        description: "Filter catalog entries by type.",
        required: false,
        example: "provider_metadata",
    },
    ParamDoc {
        name: "uri",
        location: "query",
        description: "Fetch one catalog entry by URI.",
        required: false,
        example: "runinator://providers/std",
    },
];
const AUTOMATION_FILTERS: &[ParamDoc] = &[
    ParamDoc {
        name: "workflow_run_id",
        location: "query",
        description: "Filter automation records for a workflow run.",
        required: false,
        example: UUID_EXAMPLE,
    },
    ParamDoc {
        name: "external_item_id",
        location: "query",
        description: "Filter automation records linked to an external item.",
        required: false,
        example: UUID_EXAMPLE,
    },
];
const GATE_FILTERS: &[ParamDoc] = &[
    ParamDoc {
        name: "workflow_run_id",
        location: "query",
        description: "Filter gates for a workflow run.",
        required: false,
        example: UUID_EXAMPLE,
    },
    ParamDoc {
        name: "status",
        location: "query",
        description: "Filter gates by open, closed, or waiting status.",
        required: false,
        example: "open",
    },
];
const IDEMPOTENCY_QUERY: &[ParamDoc] = &[
    ParamDoc {
        name: "scope",
        location: "query",
        description: "Namespace for the idempotency key.",
        required: true,
        example: "github-webhooks",
    },
    ParamDoc {
        name: "key",
        location: "query",
        description: "Caller-provided idempotency key.",
        required: true,
        example: "delivery-123",
    },
];
const CREDENTIAL_QUERY: &[ParamDoc] = &[
    ParamDoc {
        name: "scope",
        location: "query",
        description: "Credential or config scope.",
        required: false,
        example: "slack",
    },
    ParamDoc {
        name: "name",
        location: "query",
        description: "Credential or config name.",
        required: false,
        example: "bot_token",
    },
    ParamDoc {
        name: "kind",
        location: "query",
        description: "Setting kind: secret or config.",
        required: false,
        example: "secret",
    },
];

const ENDPOINT_DOCS: &[EndpointDoc] = &[
    endpoint(
        "get",
        "/health",
        "Meta",
        "Check service health",
        "Returns a lightweight liveness response. This endpoint is public and does not touch the database.",
        true,
        None,
        &[],
        200,
        "service is alive",
        Example::Health,
    ),
    endpoint(
        "get",
        "/ready",
        "Meta",
        "Check service readiness",
        "Verifies that the web service can answer readiness checks, including database-dependent readiness.",
        true,
        None,
        &[],
        200,
        "service is ready",
        Example::Ready,
    ),
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
    endpoint(
        "get",
        "/ws/events",
        "WebSockets",
        "Subscribe to UI events",
        "Upgrades to a websocket stream of fan-out UI events emitted by this web-service replica.",
        false,
        None,
        &[],
        101,
        "websocket upgrade accepted",
        Example::None,
    ),
    endpoint(
        "get",
        "/ws/workflow-runs/{id}",
        "WebSockets",
        "Subscribe to one workflow run",
        "Upgrades to a websocket stream for workflow-run changes and node activity for one run.",
        false,
        None,
        &[],
        101,
        "websocket upgrade accepted",
        Example::None,
    ),
    endpoint(
        "get",
        "/ws/run-stream/{id}",
        "WebSockets",
        "Subscribe to task run output",
        "Upgrades to a websocket stream for chunks emitted by one low-level task run.",
        false,
        None,
        &[],
        101,
        "websocket upgrade accepted",
        Example::None,
    ),
    endpoint(
        "get",
        "/ws/workflow-node-runs/{id}/stream",
        "WebSockets",
        "Subscribe to node-run output",
        "Upgrades to a websocket stream for chunks emitted by one workflow node run.",
        false,
        None,
        &[],
        101,
        "websocket upgrade accepted",
        Example::None,
    ),
    endpoint(
        "get",
        "/workflows",
        "Workflows",
        "List workflow definitions",
        "Lists workflow definitions visible to the caller. Supplying `name` returns the matching workflow instead of the full list.",
        false,
        None,
        WORKFLOW_FILTERS,
        200,
        "workflow definitions",
        Example::WorkflowList,
    ),
    endpoint(
        "post",
        "/workflows",
        "Workflows",
        "Create or replace a workflow",
        "Stores a workflow definition. New workflows are owned by the creator; updating an existing workflow requires edit access.",
        false,
        json_body(
            "Workflow definition to create or replace.",
            Example::Workflow,
        ),
        &[],
        200,
        "stored workflow definition",
        Example::Workflow,
    ),
    endpoint(
        "post",
        "/workflows/validate",
        "Workflows",
        "Validate a workflow definition",
        "Validates a workflow against graph, typing, provider, and config rules without saving it.",
        false,
        json_body("Workflow definition to validate.", Example::Workflow),
        &[],
        200,
        "validated workflow definition",
        Example::Workflow,
    ),
    endpoint(
        "post",
        "/wdl/complete",
        "WDL",
        "Complete WDL source",
        "Returns editor completions for a WDL source buffer and cursor position.",
        false,
        json_body(
            "Completion request with WDL source and cursor position.",
            Example::WdlCompletion,
        ),
        &[],
        200,
        "completion candidates",
        Example::WdlCompletion,
    ),
    endpoint(
        "post",
        "/wdl/hover",
        "WDL",
        "Hover WDL source",
        "Returns editor hover documentation and type information for a WDL source buffer and cursor position.",
        false,
        json_body(
            "Hover request with WDL source, cursor byte offset, and optional metadata.",
            Example::WdlCompletion,
        ),
        &[],
        200,
        "hover information",
        Example::WdlHover,
    ),
    endpoint(
        "post",
        "/wdl/compile",
        "WDL",
        "Compile WDL source",
        "Compiles WDL into a workflow definition using registered provider metadata for validation.",
        false,
        json_body("WDL source and initial enabled flag.", Example::WdlCompile),
        &[],
        200,
        "compiled workflow definition",
        Example::Workflow,
    ),
    endpoint(
        "post",
        "/wdl/analyze",
        "WDL",
        "Analyze WDL source",
        "Returns parser, semantic, and provider-aware diagnostics for a WDL source buffer or fragment.",
        false,
        json_body(
            "WDL source, optionally scoped to a fragment kind.",
            Example::WdlSource,
        ),
        &[],
        200,
        "diagnostics",
        Example::WdlDiagnostics,
    ),
    endpoint(
        "post",
        "/wdl/format",
        "WDL",
        "Format WDL source",
        "Formats WDL source text and returns the formatted source string.",
        false,
        json_body("WDL source to format.", Example::WdlSource),
        &[],
        200,
        "formatted source",
        Example::WdlSource,
    ),
    endpoint(
        "post",
        "/wdl/decompile",
        "WDL",
        "Decompile workflow JSON to WDL",
        "Converts a workflow definition back into WDL source when the graph can be represented by the language.",
        false,
        json_body(
            "Workflow definition to render as WDL.",
            Example::WdlDecompile,
        ),
        &[],
        200,
        "WDL source",
        Example::WdlSource,
    ),
    endpoint(
        "post",
        "/wdl/evaluate",
        "WDL",
        "Evaluate a WDL expression or fragment",
        "Evaluates a pure WDL expression, condition, or compute fragment against a supplied preview context.",
        false,
        json_body(
            "Expression or fragment source plus context.",
            Example::WdlEvaluate,
        ),
        &[],
        200,
        "evaluated value",
        Example::WdlEvaluate,
    ),
    endpoint(
        "post",
        "/wdl/import",
        "WDL",
        "Compile and import WDL",
        "Compiles WDL source client-style on the web service path used by the command center, then imports the resulting workflow bundle.",
        false,
        json_body(
            "WDL source, target workflow id, triggers, and UI metadata.",
            Example::WdlCompile,
        ),
        &[],
        200,
        "imported workflow bundle",
        Example::WorkflowBundle,
    ),
    endpoint(
        "post",
        "/packs/import",
        "Packs",
        "Import a compiled pack zip",
        "Imports a compiled `.wdlm`/pack zip containing `workflows.json` and optional `secrets.json`. The backend reads compiled JSON only; it does not compile WDL.",
        false,
        Some(RequestDoc {
            description: "Compiled pack zip, or JSON in compatibility mode.",
            example: Example::WorkflowBundle,
            content_type: "application/zip",
        }),
        PACK_IMPORT_PARAMS,
        200,
        "pack import result",
        Example::WorkflowBundle,
    ),
    endpoint(
        "post",
        "/workflows/import",
        "Packs",
        "Import a raw workflow bundle",
        "Legacy JSON bundle import. This is intentionally guarded because raw JSON can bypass WDL well-formedness constraints.",
        false,
        json_body("Raw workflow bundle JSON.", Example::WorkflowBundle),
        WORKFLOW_IMPORT_HEADERS,
        200,
        "imported workflow bundle",
        Example::WorkflowBundle,
    ),
    endpoint(
        "get",
        "/workflows/export",
        "Packs",
        "Export visible workflows",
        "Exports the caller's visible workflow definitions and triggers as a JSON workflow bundle.",
        false,
        None,
        &[],
        200,
        "workflow bundle",
        Example::WorkflowBundle,
    ),
    endpoint(
        "get",
        "/workflows/{id}",
        "Workflows",
        "Get a workflow",
        "Fetches one workflow definition by id if the caller has view access.",
        false,
        None,
        &[],
        200,
        "workflow definition",
        Example::Workflow,
    ),
    endpoint(
        "patch",
        "/workflows/{id}",
        "Workflows",
        "Update a workflow",
        "Replaces the stored workflow definition for the id in the path. The request body should carry the full workflow definition.",
        false,
        json_body("Workflow definition to store.", Example::Workflow),
        &[],
        200,
        "updated workflow definition",
        Example::Workflow,
    ),
    endpoint(
        "delete",
        "/workflows/{id}",
        "Workflows",
        "Delete a workflow",
        "Deletes a workflow definition. The caller must have edit access.",
        false,
        None,
        &[],
        200,
        "workflow deleted",
        Example::TaskResponse,
    ),
    endpoint(
        "get",
        "/workflows/{id}/export",
        "Packs",
        "Export one workflow",
        "Exports one workflow definition and its triggers as a JSON workflow bundle.",
        false,
        None,
        &[],
        200,
        "workflow bundle",
        Example::WorkflowBundle,
    ),
    endpoint(
        "post",
        "/workflows/{id}/duplicate",
        "Workflows",
        "Duplicate a workflow",
        "Creates a copy of a workflow. The optional `bump` query in the model controls version bump behavior.",
        false,
        None,
        &[],
        200,
        "duplicated workflow",
        Example::Workflow,
    ),
    endpoint(
        "get",
        "/workflows/{id}/triggers",
        "Workflows",
        "List workflow triggers",
        "Lists triggers attached to one workflow definition.",
        false,
        None,
        &[],
        200,
        "workflow triggers",
        Example::TriggerList,
    ),
    endpoint(
        "post",
        "/workflows/{id}/triggers",
        "Workflows",
        "Create or replace a workflow trigger",
        "Creates or upserts a trigger for the workflow definition in the path.",
        false,
        json_body("Workflow trigger definition.", Example::Trigger),
        &[],
        200,
        "stored workflow trigger",
        Example::Trigger,
    ),
    endpoint(
        "get",
        "/workflow_triggers/due",
        "Control Plane",
        "List due workflow triggers",
        "Returns workflow triggers that are ready to fire. Used by scheduler loops and diagnostics.",
        false,
        None,
        WORKFLOW_TRIGGER_FILTERS,
        200,
        "due workflow triggers",
        Example::TriggerList,
    ),
    endpoint(
        "post",
        "/scheduler/workflow_trigger_firings/claim",
        "Control Plane",
        "Claim due trigger firings",
        "Service-control endpoint used by schedulers to claim due workflow-trigger firings with a lease.",
        false,
        json_body("Scheduler id and claim limit.", Example::TriggerClaim),
        &[],
        200,
        "claimed trigger firings",
        Example::TriggerList,
    ),
    endpoint(
        "get",
        "/workflow_triggers/{id}",
        "Workflows",
        "Get a workflow trigger",
        "Fetches one workflow trigger by id.",
        false,
        None,
        &[],
        200,
        "workflow trigger",
        Example::Trigger,
    ),
    endpoint(
        "patch",
        "/workflow_triggers/{id}",
        "Workflows",
        "Update a workflow trigger",
        "Updates one workflow trigger by id.",
        false,
        json_body("Workflow trigger fields to store.", Example::Trigger),
        &[],
        200,
        "updated workflow trigger",
        Example::Trigger,
    ),
    endpoint(
        "delete",
        "/workflow_triggers/{id}",
        "Workflows",
        "Delete a workflow trigger",
        "Deletes one workflow trigger by id.",
        false,
        None,
        &[],
        200,
        "workflow trigger deleted",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_triggers/{id}/runs",
        "Workflow Runs",
        "Start a run from a trigger",
        "Creates a workflow run using a trigger id and the supplied parameters.",
        false,
        json_body("Trigger run parameters.", Example::WorkflowRunRequest),
        &[],
        202,
        "workflow run accepted",
        Example::WorkflowRun,
    ),
    endpoint(
        "get",
        "/workflow_runs",
        "Workflow Runs",
        "List workflow runs",
        "Lists recent workflow runs visible to the caller, with optional filters by status, workflow id, name, or open state.",
        false,
        None,
        WORKFLOW_RUN_FILTERS,
        200,
        "workflow runs",
        Example::WorkflowRunList,
    ),
    endpoint(
        "get",
        "/replicas",
        "Replicas",
        "List service replicas",
        "Lists registered web, worker, waker, scheduler, and other runtime replicas, optionally filtered by kind or status.",
        false,
        None,
        REPLICA_FILTERS,
        200,
        "replicas",
        Example::ReplicaList,
    ),
    endpoint(
        "post",
        "/replicas/register",
        "Replicas",
        "Register a replica",
        "Registers a runtime replica and its advertised identity.",
        false,
        json_body("Replica registration record.", Example::Replica),
        &[],
        200,
        "registered replica",
        Example::Replica,
    ),
    endpoint(
        "post",
        "/replicas/{replica_id}/heartbeat",
        "Replicas",
        "Heartbeat a replica",
        "Updates a replica heartbeat and status so the service can track liveness.",
        false,
        json_body("Replica heartbeat fields.", Example::Replica),
        &[],
        200,
        "heartbeat recorded",
        Example::Replica,
    ),
    endpoint(
        "post",
        "/replicas/{replica_id}/offline",
        "Replicas",
        "Mark a replica offline",
        "Marks a registered replica offline.",
        false,
        None,
        &[],
        200,
        "replica marked offline",
        Example::TaskResponse,
    ),
    endpoint(
        "get",
        "/replicas/{replica_id}/providers",
        "Replicas",
        "List replica providers",
        "Lists provider registrations advertised by one replica.",
        false,
        None,
        &[],
        200,
        "replica providers",
        Example::ProviderList,
    ),
    endpoint(
        "post",
        "/replicas/{replica_id}/providers",
        "Replicas",
        "Upsert a replica provider",
        "Stores provider metadata advertised by one replica.",
        false,
        json_body("Replica provider registration.", Example::ReplicaProvider),
        &[],
        200,
        "replica provider stored",
        Example::ReplicaProvider,
    ),
    endpoint(
        "post",
        "/scheduler/workflow_runs/claim",
        "Control Plane",
        "Claim workflow runs for scheduling",
        "Service-control endpoint used by scheduler loops to claim runnable workflow runs with a lease.",
        false,
        json_body(
            "Scheduler id, lease deadline, statuses, and limit.",
            Example::SchedulerRunClaim,
        ),
        &[],
        200,
        "claimed workflow runs",
        Example::WorkflowRunList,
    ),
    endpoint(
        "post",
        "/scheduler/ready_nodes/claim",
        "Control Plane",
        "Claim ready nodes",
        "Service-control endpoint used by scheduler loops to claim ready workflow nodes before dispatching wakes or actions.",
        false,
        json_body(
            "Scheduler id, lease deadline, and limit.",
            Example::SchedulerReadyNodeClaim,
        ),
        &[],
        200,
        "claimed ready nodes",
        Example::WorkflowRunList,
    ),
    endpoint(
        "get",
        "/runs",
        "Runs",
        "List task runs by status",
        "Service-control endpoint that lists low-level task runs for a required status.",
        false,
        None,
        RUN_FILTERS,
        200,
        "task runs",
        Example::RunList,
    ),
    endpoint(
        "patch",
        "/runs/{id}",
        "Runs",
        "Update a task run",
        "Service-control endpoint used by workers to update low-level task-run status and output.",
        false,
        json_body("Task run status update.", Example::RunStatus),
        &[],
        200,
        "task run updated",
        Example::TaskResponse,
    ),
    endpoint(
        "get",
        "/runs/{id}/chunks",
        "Runs",
        "List task run chunks",
        "Service-control endpoint that returns streamed chunks for a low-level task run.",
        false,
        None,
        CURSOR,
        200,
        "task run chunks",
        Example::RunChunk,
    ),
    endpoint(
        "post",
        "/runs/{id}/chunks",
        "Runs",
        "Append a task run chunk",
        "Service-control endpoint used by workers to append stdout, stderr, log, or structured chunks.",
        false,
        json_body("Run chunk to append.", Example::RunChunk),
        &[],
        202,
        "task run chunk appended",
        Example::RunChunk,
    ),
    endpoint(
        "get",
        "/runs/{id}/artifacts",
        "Artifacts",
        "List task run artifacts",
        "Lists artifacts linked to a low-level task run.",
        false,
        None,
        &[],
        200,
        "run artifacts",
        Example::Artifact,
    ),
    endpoint(
        "post",
        "/runs/{id}/artifacts",
        "Artifacts",
        "Attach a task run artifact",
        "Registers an artifact produced by a low-level task run.",
        false,
        json_body("Artifact metadata to attach.", Example::Artifact),
        &[],
        202,
        "run artifact attached",
        Example::Artifact,
    ),
    endpoint(
        "get",
        "/artifacts",
        "Artifacts",
        "List artifacts",
        "Lists stored artifacts across runs when permitted by the caller.",
        false,
        None,
        &[],
        200,
        "artifacts",
        Example::Artifact,
    ),
    endpoint(
        "post",
        "/artifacts/upload",
        "Artifacts",
        "Upload artifact bytes",
        "Uploads artifact content as multipart form data and records artifact metadata.",
        false,
        Some(RequestDoc {
            description: "Multipart artifact upload payload.",
            example: Example::Artifact,
            content_type: "multipart/form-data",
        }),
        &[],
        200,
        "artifact uploaded",
        Example::Artifact,
    ),
    endpoint(
        "get",
        "/artifacts/{id}/download",
        "Artifacts",
        "Download an artifact",
        "Downloads artifact bytes for the requested artifact id.",
        false,
        None,
        &[],
        200,
        "artifact bytes",
        Example::None,
    ),
    endpoint(
        "get",
        "/notifications",
        "Notifications",
        "List notifications",
        "Lists notifications for the current principal.",
        false,
        None,
        &[],
        200,
        "notifications",
        Example::NotificationList,
    ),
    endpoint(
        "post",
        "/notifications",
        "Notifications",
        "Create a notification",
        "Creates a notification record.",
        false,
        json_body("Notification payload.", Example::Notification),
        &[],
        200,
        "created notification",
        Example::Notification,
    ),
    endpoint(
        "post",
        "/notifications/{id}/mark_read",
        "Notifications",
        "Mark a notification read",
        "Marks one notification as read.",
        false,
        None,
        &[],
        200,
        "notification marked read",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/notifications/mark_all_read",
        "Notifications",
        "Mark all notifications read",
        "Marks all notifications visible to the caller as read.",
        false,
        None,
        &[],
        200,
        "notifications marked read",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflows/{id}/runs",
        "Workflow Runs",
        "Start a workflow run",
        "Creates a workflow run from a workflow definition and supplied parameters.",
        false,
        json_body("Workflow run parameters.", Example::WorkflowRunRequest),
        &[],
        202,
        "workflow run accepted",
        Example::WorkflowRun,
    ),
    endpoint(
        "get",
        "/workflow_runs/{id}",
        "Workflow Runs",
        "Get a workflow run",
        "Fetches a workflow run plus node-run records.",
        false,
        None,
        &[],
        200,
        "workflow run",
        Example::WorkflowRun,
    ),
    endpoint(
        "patch",
        "/workflow_runs/{id}",
        "Control Plane",
        "Update a workflow run",
        "Service-control endpoint used by runtime loops to update workflow-run status, state, and active node.",
        false,
        json_body("Workflow run status update.", Example::WorkflowRunStatus),
        &[],
        200,
        "workflow run updated",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/scheduler/workflow_runs/{id}/claim/renew",
        "Control Plane",
        "Renew a workflow-run claim",
        "Renews a scheduler lease for a claimed workflow run.",
        false,
        json_body(
            "Scheduler id and new lease deadline.",
            Example::SchedulerRunLease,
        ),
        &[],
        200,
        "workflow-run claim renewed",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/scheduler/workflow_runs/{id}/claim/release",
        "Control Plane",
        "Release a workflow-run claim",
        "Releases a scheduler lease for a claimed workflow run.",
        false,
        json_body(
            "Scheduler id releasing the claim.",
            Example::SchedulerRunLease,
        ),
        &[],
        200,
        "workflow-run claim released",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/scheduler/action_dispatches",
        "Control Plane",
        "Enqueue an action dispatch",
        "Durable outbox endpoint for scheduling an action command that a worker will execute.",
        false,
        json_body("Action dispatch record.", Example::ActionDispatch),
        &[],
        200,
        "action dispatch queued",
        Example::ActionDispatch,
    ),
    endpoint(
        "get",
        "/scheduler/action_dispatches/pending",
        "Control Plane",
        "List pending action dispatches",
        "Lists action dispatches waiting to be published to the broker action channel.",
        false,
        None,
        &[],
        200,
        "pending action dispatches",
        Example::ActionDispatchList,
    ),
    endpoint(
        "post",
        "/scheduler/action_dispatches/claim",
        "Control Plane",
        "Claim action dispatches",
        "Claims pending action dispatches for the action publisher loop.",
        false,
        json_body("Claim owner and limit.", Example::ActionDispatch),
        &[],
        200,
        "claimed action dispatches",
        Example::ActionDispatchList,
    ),
    endpoint(
        "post",
        "/scheduler/action_dispatches/{id}/published",
        "Control Plane",
        "Mark action dispatch published",
        "Marks an action dispatch as successfully published to the broker.",
        false,
        None,
        &[],
        200,
        "action dispatch marked published",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/scheduler/action_dispatches/{id}/failed",
        "Control Plane",
        "Mark action dispatch failed",
        "Records a publish failure for an action dispatch.",
        false,
        json_body("Failure detail.", Example::ActionDispatch),
        &[],
        200,
        "action dispatch marked failed",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/scheduler/ready_nodes/{id}/process",
        "Control Plane",
        "Complete a ready-node claim",
        "Marks a ready node processed and optionally records the next wake that should be scheduled.",
        false,
        json_body("Ready-node completion payload.", Example::ReadyNodeProcess),
        &[],
        200,
        "ready node processed",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/debug/command",
        "Debug",
        "Run a debugger command",
        "Applies a debugger command to a paused or debuggable workflow run.",
        false,
        json_body("Debugger command payload.", Example::AutomationRecord),
        &[],
        200,
        "debug command applied",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/debug/step",
        "Debug",
        "Step a workflow run",
        "Advances a debug-paused workflow run by one reducer step.",
        false,
        None,
        &[],
        200,
        "debug step applied",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/debug/continue",
        "Debug",
        "Continue a workflow run",
        "Continues a debug-paused workflow run.",
        false,
        None,
        &[],
        200,
        "workflow run continued",
        Example::TaskResponse,
    ),
    endpoint(
        "patch",
        "/workflow_runs/{id}/debug",
        "Debug",
        "Update debugger state",
        "Updates debugger flags or breakpoints for a workflow run.",
        false,
        json_body("Debug state patch.", Example::AutomationRecord),
        &[],
        200,
        "debug state updated",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/cancel",
        "Workflow Runs",
        "Cancel a workflow run",
        "Requests cancellation for a workflow run and publishes the required runtime control signals.",
        false,
        None,
        &[],
        200,
        "workflow run cancel requested",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/pause",
        "Workflow Runs",
        "Pause a workflow run",
        "Requests that the reducer pause a workflow run at a safe runtime boundary.",
        false,
        None,
        &[],
        200,
        "workflow run pause requested",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/resume",
        "Workflow Runs",
        "Resume a workflow run",
        "Requests that a paused workflow run resume execution.",
        false,
        None,
        &[],
        200,
        "workflow run resume requested",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/signals",
        "Workflow Runs",
        "Deliver a signal to a run",
        "Delivers an external signal payload to a parked node in one workflow run.",
        false,
        json_body("Signal name and payload.", Example::WebhookSignal),
        &[],
        200,
        "signal delivered",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/debug/run_to_cursor",
        "Debug",
        "Run to debugger cursor",
        "Continues a debug-paused run until the requested node or breakpoint is reached.",
        false,
        json_body("Debugger cursor target.", Example::AutomationRecord),
        &[],
        200,
        "run-to-cursor started",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/debug/skip",
        "Debug",
        "Skip a debug node",
        "Skips the selected node while debugging a workflow run.",
        false,
        json_body("Node skip request.", Example::AutomationRecord),
        &[],
        200,
        "debug node skipped",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/debug/rerun_node",
        "Debug",
        "Rerun a debug node",
        "Reruns a selected node while debugging a workflow run.",
        false,
        json_body("Node rerun request.", Example::AutomationRecord),
        &[],
        200,
        "debug node rerun requested",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/replay",
        "Workflow Runs",
        "Replay a workflow run",
        "Creates a replay of a workflow run, optionally starting from a specific node id.",
        false,
        json_body("Optional replay start node.", Example::WorkflowRunReplay),
        &[],
        202,
        "workflow run replay accepted",
        Example::WorkflowRun,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/rename",
        "Workflow Runs",
        "Rename a workflow run",
        "Sets or clears the human-readable name of a workflow run.",
        false,
        json_body(
            "New workflow-run name; null clears it.",
            Example::WorkflowRunRename,
        ),
        &[],
        200,
        "workflow run renamed",
        Example::TaskResponse,
    ),
    endpoint(
        "get",
        "/supervisor/status",
        "Supervisor",
        "Get local supervisor status",
        "Returns status for the local supervisor stack when the web service is running under it.",
        false,
        None,
        &[],
        200,
        "supervisor status",
        Example::Supervisor,
    ),
    endpoint(
        "post",
        "/workflow_runs/{id}/nodes",
        "Control Plane",
        "Create a workflow node run",
        "Service-control endpoint used by the reducer to create a node-run record.",
        false,
        json_body("Node-run creation payload.", Example::NodeRun),
        &[],
        200,
        "workflow node run",
        Example::NodeRun,
    ),
    endpoint(
        "post",
        "/workflow_node_runs/{id}/claim",
        "Control Plane",
        "Claim a node run for execution",
        "Worker-control endpoint used to claim a node run before executing the provider action.",
        false,
        json_body("Executor claim payload.", Example::NodeRunClaim),
        &[],
        200,
        "node run claimed",
        Example::NodeRun,
    ),
    endpoint(
        "post",
        "/workflow_node_runs/{id}/release",
        "Control Plane",
        "Release a node-run claim",
        "Worker-control endpoint used to release a node-run execution claim.",
        false,
        json_body("Executor release payload.", Example::NodeRunRelease),
        &[],
        200,
        "node-run claim released",
        Example::TaskResponse,
    ),
    endpoint(
        "patch",
        "/workflow_node_runs/{id}",
        "Control Plane",
        "Update a workflow node run",
        "Worker-control endpoint used to update node-run status, attempt, parameters, output, state, or message.",
        false,
        json_body("Node-run status update.", Example::NodeRunStatus),
        &[],
        200,
        "node run updated",
        Example::NodeRun,
    ),
    endpoint(
        "post",
        "/workflow_node_runs/{id}/input",
        "Control Plane",
        "Resolve a node-run input",
        "Records a human or external input resolution for a node run waiting on input.",
        false,
        json_body("Resolved input payload.", Example::NodeRunInput),
        &[],
        200,
        "node-run input resolved",
        Example::NodeRun,
    ),
    endpoint(
        "get",
        "/workflow_node_runs/{id}/chunks",
        "Control Plane",
        "List node-run chunks",
        "Returns streamed chunks for a workflow node run.",
        false,
        None,
        CURSOR,
        200,
        "node-run chunks",
        Example::RunChunk,
    ),
    endpoint(
        "post",
        "/workflow_node_runs/{id}/chunks",
        "Control Plane",
        "Append a node-run chunk",
        "Appends stdout, stderr, log, or structured chunks for a workflow node run.",
        false,
        json_body("Node-run chunk to append.", Example::RunChunk),
        &[],
        202,
        "node-run chunk appended",
        Example::RunChunk,
    ),
    endpoint(
        "get",
        "/workflow_node_runs/{id}/artifacts",
        "Artifacts",
        "List node-run artifacts",
        "Lists artifacts attached to one workflow node run.",
        false,
        None,
        &[],
        200,
        "node-run artifacts",
        Example::Artifact,
    ),
    endpoint(
        "post",
        "/workflow_node_runs/{id}/artifacts",
        "Artifacts",
        "Attach a node-run artifact",
        "Registers an artifact produced by one workflow node run.",
        false,
        json_body("Artifact metadata to attach.", Example::Artifact),
        &[],
        202,
        "node-run artifact attached",
        Example::Artifact,
    ),
    endpoint(
        "get",
        "/workflow_runs/{id}/artifacts",
        "Workflow Runs",
        "List workflow run artifacts",
        "Lists artifacts declared by output nodes in one workflow run.",
        false,
        None,
        &[],
        200,
        "workflow run artifacts",
        Example::ArtifactList,
    ),
    endpoint(
        "get",
        "/catalog/items",
        "Catalog",
        "List catalog items",
        "Lists catalog entries such as provider metadata used by authoring clients.",
        false,
        None,
        CATALOG_FILTERS,
        200,
        "catalog items",
        Example::CatalogItem,
    ),
    endpoint(
        "post",
        "/catalog/items",
        "Catalog",
        "Upsert a catalog item",
        "Creates or replaces a catalog entry.",
        false,
        json_body("Catalog item payload.", Example::CatalogItem),
        &[],
        200,
        "catalog item stored",
        Example::CatalogItem,
    ),
    endpoint(
        "get",
        "/external_items",
        "Automation",
        "List external items",
        "Lists external automation records, optionally filtered by workflow run or linked item.",
        false,
        None,
        AUTOMATION_FILTERS,
        200,
        "external items",
        Example::AutomationRecord,
    ),
    endpoint(
        "post",
        "/external_items",
        "Automation",
        "Create an external item",
        "Creates an external automation record. Service credentials or admin privileges are required.",
        false,
        json_body("External item record.", Example::AutomationRecord),
        &[],
        202,
        "external item created",
        Example::AutomationRecord,
    ),
    endpoint(
        "get",
        "/gates",
        "Automation",
        "List gates",
        "Lists gate records, optionally filtered by workflow run or status.",
        false,
        None,
        GATE_FILTERS,
        200,
        "gates",
        Example::AutomationRecord,
    ),
    endpoint(
        "post",
        "/gates",
        "Automation",
        "Create a gate",
        "Creates a gate automation record. Service credentials or admin privileges are required.",
        false,
        json_body("Gate record.", Example::AutomationRecord),
        &[],
        202,
        "gate created",
        Example::AutomationRecord,
    ),
    endpoint(
        "get",
        "/gates/{id}",
        "Automation",
        "Get a gate",
        "Fetches one gate record by id if the caller can view the owning workflow.",
        false,
        None,
        &[],
        200,
        "gate",
        Example::AutomationRecord,
    ),
    endpoint(
        "post",
        "/gates/{id}/open",
        "Automation",
        "Open a gate",
        "Resolves a gate as open and unblocks any reducer path waiting on it.",
        false,
        json_body("Gate resolution metadata.", Example::GateResolution),
        &[],
        200,
        "gate opened",
        Example::AutomationRecord,
    ),
    endpoint(
        "post",
        "/gates/{id}/close",
        "Automation",
        "Close a gate",
        "Resolves a gate as closed and unblocks any reducer path waiting on it.",
        false,
        json_body("Gate resolution metadata.", Example::GateResolution),
        &[],
        200,
        "gate closed",
        Example::AutomationRecord,
    ),
    endpoint(
        "get",
        "/automation_events",
        "Automation",
        "List automation events",
        "Lists generic automation event records.",
        false,
        None,
        AUTOMATION_FILTERS,
        200,
        "automation events",
        Example::AutomationRecord,
    ),
    endpoint(
        "post",
        "/automation_events",
        "Automation",
        "Create an automation event",
        "Creates a generic automation event record. Service credentials or admin privileges are required.",
        false,
        json_body("Automation event record.", Example::AutomationRecord),
        &[],
        202,
        "automation event created",
        Example::AutomationRecord,
    ),
    endpoint(
        "get",
        "/approvals",
        "Automation",
        "List approval requests",
        "Lists approval request records, optionally filtered by workflow run or linked item.",
        false,
        None,
        AUTOMATION_FILTERS,
        200,
        "approval requests",
        Example::AutomationRecord,
    ),
    endpoint(
        "post",
        "/approvals",
        "Automation",
        "Create an approval request",
        "Creates an approval request record. Service credentials or admin privileges are required.",
        false,
        json_body("Approval request record.", Example::AutomationRecord),
        &[],
        202,
        "approval request created",
        Example::AutomationRecord,
    ),
    endpoint(
        "post",
        "/approvals/{id}/approve",
        "Automation",
        "Approve a request",
        "Resolves an approval request as approved and stores optional output.",
        false,
        json_body("Approval resolution payload.", Example::ApprovalResolution),
        &[],
        200,
        "approval request approved",
        Example::AutomationRecord,
    ),
    endpoint(
        "post",
        "/approvals/{id}/reject",
        "Automation",
        "Reject a request",
        "Resolves an approval request as rejected and stores optional output.",
        false,
        json_body("Approval resolution payload.", Example::ApprovalResolution),
        &[],
        200,
        "approval request rejected",
        Example::AutomationRecord,
    ),
    endpoint(
        "get",
        "/idempotency_keys",
        "Control Plane",
        "Get an idempotency key",
        "Fetches a stored idempotency result by scope and key. Service credentials or admin privileges are required.",
        false,
        None,
        IDEMPOTENCY_QUERY,
        200,
        "idempotency result",
        Example::Idempotency,
    ),
    endpoint(
        "post",
        "/idempotency_keys",
        "Control Plane",
        "Put an idempotency key",
        "Stores an idempotency result for later duplicate-request suppression.",
        false,
        json_body("Idempotency scope, key, and result.", Example::Idempotency),
        &[],
        200,
        "idempotency key stored",
        Example::Idempotency,
    ),
    endpoint(
        "get",
        "/credentials",
        "Credentials",
        "Get credentials or config",
        "Fetches a credential/config entry or lists entries by scope/kind. Secret values remain protected by the credential store behavior.",
        false,
        None,
        CREDENTIAL_QUERY,
        200,
        "credential metadata",
        Example::Credential,
    ),
    endpoint(
        "post",
        "/credentials",
        "Credentials",
        "Store a credential or config value",
        "Stores a secret or typed config value. Config values carry or infer a JSON schema pinned for future updates.",
        false,
        json_body("Credential or config value.", Example::Credential),
        &[],
        200,
        "credential stored",
        Example::Credential,
    ),
    endpoint(
        "delete",
        "/credentials",
        "Credentials",
        "Delete a credential or config value",
        "Deletes a secret or config setting selected by query parameters.",
        false,
        None,
        CREDENTIAL_QUERY,
        200,
        "credential deleted",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/credentials/import",
        "Credentials",
        "Import a secret bundle",
        "Imports secret/config entries from a compiled pack secret bundle.",
        false,
        json_body("Secret bundle to import.", Example::SecretBundle),
        &[],
        200,
        "secret bundle imported",
        Example::SecretBundle,
    ),
    endpoint(
        "post",
        "/credentials/reencrypt",
        "Credentials",
        "Re-encrypt stored settings",
        "Re-encrypts stored secrets/config values after credential-store rotation.",
        false,
        None,
        &[],
        200,
        "settings re-encrypted",
        Example::TaskResponse,
    ),
    endpoint(
        "get",
        "/providers",
        "Providers",
        "List providers",
        "Lists registered provider metadata used by workers and workflow authoring.",
        false,
        None,
        &[],
        200,
        "providers",
        Example::ProviderList,
    ),
    endpoint(
        "post",
        "/providers",
        "Providers",
        "Upsert a provider",
        "Stores provider metadata for a provider implementation.",
        false,
        json_body("Provider metadata.", Example::Provider),
        &[],
        200,
        "provider stored",
        Example::Provider,
    ),
    endpoint(
        "post",
        "/providers/import",
        "Providers",
        "Import provider bundle",
        "Imports provider metadata from a provider bundle.",
        false,
        json_body("Provider bundle.", Example::ProviderBundle),
        &[],
        200,
        "provider bundle imported",
        Example::ProviderBundle,
    ),
    endpoint(
        "post",
        "/webhooks/wake",
        "Webhooks",
        "Drive a waiting run by webhook",
        "External webhook ingress that wakes or updates a parked workflow node by run id.",
        false,
        json_body("Webhook wake payload.", Example::WebhookWake),
        &[],
        202,
        "webhook accepted",
        Example::TaskResponse,
    ),
    endpoint(
        "post",
        "/webhooks/signal",
        "Webhooks",
        "Deliver a signal by correlation key",
        "External webhook ingress that routes a signal to a parked node by business correlation key.",
        false,
        json_body("Webhook signal payload.", Example::WebhookSignal),
        &[],
        202,
        "signal accepted",
        Example::TaskResponse,
    ),
    endpoint(
        "get",
        "/auth/config",
        "Auth",
        "Read auth configuration",
        "Public endpoint that tells clients whether authentication is enabled.",
        true,
        None,
        &[],
        200,
        "auth configuration",
        Example::AuthConfig,
    ),
    endpoint(
        "post",
        "/auth/login",
        "Auth",
        "Log in",
        "Exchanges a local username and password for an access token and refresh token.",
        true,
        json_body("Username and password.", Example::LoginRequest),
        &[],
        200,
        "token pair",
        Example::LoginResponse,
    ),
    endpoint(
        "post",
        "/auth/refresh",
        "Auth",
        "Refresh a session",
        "Rotates a refresh token and returns a new access token, refresh token, and user record.",
        true,
        json_body("Refresh token to rotate.", Example::RefreshRequest),
        &[],
        200,
        "rotated token pair",
        Example::LoginResponse,
    ),
    endpoint(
        "post",
        "/auth/logout",
        "Auth",
        "Log out",
        "Revokes a refresh token. The response is successful even if the token is already gone.",
        false,
        json_body("Refresh token to revoke.", Example::RefreshRequest),
        &[],
        200,
        "refresh session revoked",
        Example::TaskResponse,
    ),
    endpoint(
        "get",
        "/auth/me",
        "Auth",
        "Get current principal",
        "Returns the current authenticated user, or a service principal marker for service API keys.",
        false,
        None,
        &[],
        200,
        "current principal",
        Example::User,
    ),
    endpoint(
        "get",
        "/users",
        "Auth",
        "List users",
        "Admin endpoint that lists local users.",
        false,
        None,
        &[],
        200,
        "users",
        Example::UserList,
    ),
    endpoint(
        "post",
        "/users",
        "Auth",
        "Create a user",
        "Admin endpoint that creates a local user and password credential.",
        false,
        json_body("User creation payload.", Example::User),
        &[],
        200,
        "created user",
        Example::User,
    ),
    endpoint(
        "patch",
        "/users/{id}",
        "Auth",
        "Update a user",
        "Admin endpoint that updates user flags, email, or password.",
        false,
        json_body("User update payload.", Example::User),
        &[],
        200,
        "updated user",
        Example::User,
    ),
    endpoint(
        "delete",
        "/users/{id}",
        "Auth",
        "Delete a user",
        "Admin endpoint that deletes a local user unless it is the last enabled admin.",
        false,
        None,
        &[],
        200,
        "user deleted",
        Example::TaskResponse,
    ),
    endpoint(
        "get",
        "/users/{id}/teams",
        "Auth",
        "List user teams",
        "Admin endpoint that lists the teams a user belongs to.",
        false,
        None,
        &[],
        200,
        "user teams",
        Example::Team,
    ),
    endpoint(
        "get",
        "/api_keys",
        "Auth",
        "List API keys",
        "Lists API keys visible to the caller. Admins see all keys; users see their own keys.",
        false,
        None,
        &[],
        200,
        "api keys",
        Example::ApiKeyList,
    ),
    endpoint(
        "post",
        "/api_keys",
        "Auth",
        "Create an API key",
        "Creates a personal or, for admins, service API key and returns the secret once.",
        false,
        json_body("API key creation payload.", Example::ApiKey),
        &[],
        200,
        "created api key and secret",
        Example::ApiKey,
    ),
    endpoint(
        "delete",
        "/api_keys/{id}",
        "Auth",
        "Revoke an API key",
        "Admin endpoint that revokes an API key.",
        false,
        None,
        &[],
        200,
        "api key revoked",
        Example::TaskResponse,
    ),
    endpoint(
        "patch",
        "/api_keys/{id}",
        "Auth",
        "Update an API key",
        "Admin endpoint that updates API key metadata such as name, expiry, or disabled state.",
        false,
        json_body("API key update payload.", Example::ApiKey),
        &[],
        200,
        "updated api key",
        Example::ApiKeyList,
    ),
    endpoint(
        "post",
        "/api_keys/{id}/rotate",
        "Auth",
        "Rotate an API key",
        "Admin endpoint that disables an API key and returns a replacement secret once.",
        false,
        None,
        &[],
        200,
        "rotated api key and secret",
        Example::ApiKey,
    ),
    endpoint(
        "get",
        "/workflows/{id}/grants",
        "Auth",
        "List workflow grants",
        "Lists sharing grants for one workflow. Owner or admin access is required.",
        false,
        None,
        &[],
        200,
        "workflow grants",
        Example::Grant,
    ),
    endpoint(
        "post",
        "/workflows/{id}/grants",
        "Auth",
        "Create a workflow grant",
        "Creates a sharing grant for one workflow. Owner or admin access is required.",
        false,
        json_body("Workflow grant payload.", Example::Grant),
        &[],
        200,
        "workflow grant created",
        Example::Grant,
    ),
    endpoint(
        "delete",
        "/workflows/{id}/grants/{grant_id}",
        "Auth",
        "Revoke a workflow grant",
        "Revokes a sharing grant for one workflow. Owner or admin access is required.",
        false,
        None,
        &[],
        200,
        "workflow grant revoked",
        Example::TaskResponse,
    ),
    endpoint(
        "get",
        "/teams",
        "Auth",
        "List teams",
        "Admin endpoint that lists teams.",
        false,
        None,
        &[],
        200,
        "teams",
        Example::Team,
    ),
    endpoint(
        "post",
        "/teams",
        "Auth",
        "Create a team",
        "Admin endpoint that creates a team.",
        false,
        json_body("Team creation payload.", Example::Team),
        &[],
        200,
        "team created",
        Example::Team,
    ),
    endpoint(
        "delete",
        "/teams/{id}",
        "Auth",
        "Delete a team",
        "Admin endpoint that deletes a team.",
        false,
        None,
        &[],
        200,
        "team deleted",
        Example::TaskResponse,
    ),
    endpoint(
        "patch",
        "/teams/{id}",
        "Auth",
        "Update a team",
        "Admin endpoint that renames a team.",
        false,
        json_body("Team update payload.", Example::Team),
        &[],
        200,
        "team updated",
        Example::Team,
    ),
    endpoint(
        "get",
        "/teams/{id}/members",
        "Auth",
        "List team members",
        "Admin endpoint that lists users assigned to a team.",
        false,
        None,
        &[],
        200,
        "team members",
        Example::UserList,
    ),
    endpoint(
        "post",
        "/teams/{id}/members",
        "Auth",
        "Add a team member",
        "Admin endpoint that adds a user to a team.",
        false,
        json_body("Team member payload.", Example::Team),
        &[],
        200,
        "member added",
        Example::TaskResponse,
    ),
    endpoint(
        "delete",
        "/teams/{id}/members/{user_id}",
        "Auth",
        "Remove a team member",
        "Admin endpoint that removes a user from a team.",
        false,
        None,
        &[],
        200,
        "member removed",
        Example::TaskResponse,
    ),
];

const fn endpoint(
    method: &'static str,
    path: &'static str,
    tag: &'static str,
    summary: &'static str,
    description: &'static str,
    public: bool,
    request: Option<RequestDoc>,
    query: &'static [ParamDoc],
    success_status: u16,
    success_description: &'static str,
    response_example: Example,
) -> EndpointDoc {
    EndpointDoc {
        method,
        path,
        tag,
        summary,
        description,
        public,
        request,
        query,
        success_status,
        success_description,
        response_example,
    }
}

const fn json_body(description: &'static str, example: Example) -> Option<RequestDoc> {
    Some(RequestDoc {
        description,
        example,
        content_type: "application/json",
    })
}

fn enrich_openapi_document(document: &mut Value) {
    let Some(paths) = document.get_mut("paths").and_then(Value::as_object_mut) else {
        return;
    };

    for doc in ENDPOINT_DOCS {
        let path_item = paths
            .entry(doc.path.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        let path_item = path_item.as_object_mut().expect("path item is an object");
        let operation = path_item
            .entry(doc.method.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        enrich_operation(operation, doc);
    }
}

fn enrich_operation(operation: &mut Value, doc: &EndpointDoc) {
    let operation = operation.as_object_mut().expect("operation is an object");
    operation.insert("tags".into(), json!([doc.tag]));
    operation.insert("summary".into(), json!(doc.summary));
    operation.insert("description".into(), json!(doc.description));
    if doc.public {
        operation.insert("security".into(), json!([]));
    }
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
    if !doc.public {
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

fn compact_json(value: Value) -> String {
    serde_json::to_string(&value).expect("example serializes")
}

fn example_value(example: Example) -> Option<Value> {
    Some(match example {
        Example::None => return None,
        Example::Health => json!({ "status": "healthy" }),
        Example::Ready => json!({ "status": "ready" }),
        Example::AuthConfig => json!({ "enabled": true }),
        Example::LoginRequest => {
            json!({ "username": "admin", "password": "correct-horse-battery-staple" })
        }
        Example::RefreshRequest => json!({ "refresh_token": "runinator-refresh-token" }),
        Example::LoginResponse => json!({
            "access_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...",
            "refresh_token": "runinator-refresh-token",
            "expires_in": 3600,
            "user": user_example(),
        }),
        Example::TaskResponse => json!({ "success": true, "message": "Accepted" }),
        Example::Workflow => workflow_example(),
        Example::WorkflowList => json!([workflow_example()]),
        Example::WorkflowBundle => {
            json!({ "workflows": [workflow_example()], "triggers": [trigger_example()] })
        }
        Example::WorkflowRunRequest => {
            json!({ "parameters": { "environment": "dev" }, "debug": false, "name": "manual smoke test" })
        }
        Example::WorkflowRun => {
            json!({ "run": workflow_run_example(), "nodes": [node_run_example()] })
        }
        Example::WorkflowRunList => json!([workflow_run_example()]),
        Example::WorkflowRunStatus => {
            json!({ "status": "running", "active_node_id": "start", "state": {}, "message": "dispatching start node" })
        }
        Example::WorkflowRunReplay => json!({ "from_step_id": "deploy" }),
        Example::WorkflowRunRename => json!({ "name": "nightly deploy" }),
        Example::RunList => json!([{ "id": UUID_EXAMPLE, "status": "running", "provider": "std" }]),
        Example::RunStatus => {
            json!({ "status": "succeeded", "output_json": { "ok": true }, "message": "completed" })
        }
        Example::RunChunk => json!([{ "cursor": 1, "stream": "stdout", "content": "hello\n" }]),
        Example::Artifact => {
            json!({ "id": UUID_EXAMPLE, "name": "report.json", "content_type": "application/json", "size": 42 })
        }
        Example::WdlSource => {
            json!({ "source": "workflow hello {\n  task echo uses std.echo\n}\n", "fragment": "expression" })
        }
        Example::WdlCompile => {
            json!({ "source": "workflow hello {\n  task echo uses std.echo\n}\n", "enabled": true })
        }
        Example::WdlCompletion => {
            json!({ "source": "workflow hello {\n  ", "cursor_byte": 19, "providers": [], "settings": [] })
        }
        Example::WdlHover => {
            json!({ "range_start_byte": 18, "range_end_byte": 24, "title": "params", "kind": "parameter root", "detail": "{ name: string }", "documentation": "Workflow input parameters." })
        }
        Example::WdlDiagnostics => {
            json!([{ "start": 0, "end": 4, "line": 1, "column": 1, "severity": "warning", "message": "example diagnostic" }])
        }
        Example::WdlDecompile => json!({ "workflow": workflow_example() }),
        Example::WdlEvaluate => {
            json!({ "source": "inputs.environment == \"prod\"", "kind": "condition", "context": { "inputs": { "environment": "dev" } } })
        }
        Example::Trigger => trigger_example(),
        Example::TriggerList => json!([trigger_example()]),
        Example::TriggerClaim => json!({ "scheduler_id": "scheduler-1", "limit": 25 }),
        Example::SchedulerRunClaim => {
            json!({ "scheduler_id": "scheduler-1", "lease_until": "2026-06-18T13:00:00Z", "statuses": ["queued", "running"], "limit": 50 })
        }
        Example::SchedulerReadyNodeClaim => {
            json!({ "scheduler_id": "scheduler-1", "lease_until": "2026-06-18T13:00:00Z", "limit": 50 })
        }
        Example::SchedulerRunLease => {
            json!({ "scheduler_id": "scheduler-1", "lease_until": "2026-06-18T13:00:00Z" })
        }
        Example::ActionDispatch => {
            json!({ "id": UUID_EXAMPLE, "workflow_run_id": UUID_EXAMPLE, "node_id": "deploy", "status": "pending" })
        }
        Example::ActionDispatchList => {
            json!([{ "id": UUID_EXAMPLE, "workflow_run_id": UUID_EXAMPLE, "node_id": "deploy", "status": "pending" }])
        }
        Example::ReadyNodeProcess => {
            json!({ "scheduler_id": "scheduler-1", "workflow_run_id": UUID_EXAMPLE, "node_id": "wait", "next_ready_at": "2026-06-18T13:00:00Z" })
        }
        Example::NodeRun => node_run_example(),
        Example::NodeRunStatus => {
            json!({ "status": "succeeded", "attempt": 1, "output_json": { "ok": true }, "message": "done" })
        }
        Example::NodeRunInput => {
            json!({ "output_json": { "approved": true }, "message": "approved by reviewer", "resolved_by": "jane" })
        }
        Example::NodeRunClaim => {
            json!({ "replica_id": UUID_EXAMPLE, "claimed_at": "2026-06-18T12:00:00Z", "stale_before": "2026-06-18T11:55:00Z" })
        }
        Example::NodeRunRelease => {
            json!({ "replica_id": UUID_EXAMPLE, "released_at": "2026-06-18T12:05:00Z" })
        }
        Example::ArtifactList => {
            json!([{ "id": UUID_EXAMPLE, "workflow_run_id": UUID_EXAMPLE, "node_id": "report", "artifact_id": UUID_EXAMPLE, "name": "summary", "mime_type": "application/pdf", "size_bytes": 1024, "uri": "s3://bucket/key", "metadata": {}, "created_at": "2026-06-22T12:00:00Z" }])
        }
        Example::CatalogItem => {
            json!({ "item_type": "provider_metadata", "uri": "runinator://providers/std", "value": provider_example() })
        }
        Example::AutomationRecord => {
            json!({ "id": UUID_EXAMPLE, "workflow_run_id": UUID_EXAMPLE, "status": "open", "payload": { "title": "Approve deploy" } })
        }
        Example::GateResolution => {
            json!({ "resolved_by": "jane", "reason": "validated release window" })
        }
        Example::ApprovalResolution => {
            json!({ "resolved_by": "jane", "message": "approved", "output_json": { "approved": true } })
        }
        Example::Idempotency => {
            json!({ "scope": "github-webhooks", "key": "delivery-123", "result": { "accepted": true } })
        }
        Example::Credential => {
            json!({ "scope": "slack", "name": "bot_token", "kind": "secret", "value": "xoxb-..." })
        }
        Example::SecretBundle => {
            json!({ "secrets": [{ "scope": "slack", "name": "bot_token", "value": "xoxb-...", "kind": "secret" }] })
        }
        Example::Provider => provider_example(),
        Example::ProviderList => json!([provider_example()]),
        Example::ProviderBundle => json!({ "providers": [provider_example()] }),
        Example::Replica => {
            json!({ "id": UUID_EXAMPLE, "replica_type": "worker", "status": "online", "address": "worker-1" })
        }
        Example::ReplicaList => {
            json!({ "replicas": [{ "id": UUID_EXAMPLE, "replica_type": "worker", "status": "online" }] })
        }
        Example::ReplicaProvider => {
            json!({ "replica_id": UUID_EXAMPLE, "provider": provider_example() })
        }
        Example::Notification => {
            json!({ "id": UUID_EXAMPLE, "title": "Workflow finished", "body": "hello-world succeeded", "read": false })
        }
        Example::NotificationList => {
            json!([{ "id": UUID_EXAMPLE, "title": "Workflow finished", "body": "hello-world succeeded", "read": false }])
        }
        Example::User => user_example(),
        Example::UserList => json!([user_example()]),
        Example::ApiKey => {
            json!({ "name": "local automation", "user_id": UUID_EXAMPLE, "is_service": false, "expires_at": null })
        }
        Example::ApiKeyList => {
            json!([{ "id": UUID_EXAMPLE, "name": "local automation", "user_id": UUID_EXAMPLE, "is_service": false, "key_prefix": "runi_live_1234", "expires_at": null, "disabled": false }])
        }
        Example::Grant => {
            json!({ "principal_type": "user", "principal_id": UUID_EXAMPLE, "permission": "view" })
        }
        Example::Team => json!({ "name": "platform", "user_id": UUID_EXAMPLE }),
        Example::WebhookWake => {
            json!({ "workflow_run_id": UUID_EXAMPLE, "node_id": "wait_for_ticket", "status": "succeeded", "state": {}, "message": "ticket closed" })
        }
        Example::WebhookSignal => {
            json!({ "name": "ticket.closed", "correlation_key": "PROJ-123", "payload": { "status": "done" } })
        }
        Example::Supervisor => {
            json!({ "running": true, "services": [{ "name": "runinator-ws", "status": "running" }] })
        }
    })
}

fn workflow_example() -> Value {
    json!({
        "id": UUID_EXAMPLE,
        "name": "hello-world",
        "namespace": "default",
        "version": "1.0.0",
        "enabled": true,
        "input_schema": { "type": "object", "properties": { "environment": { "type": "string" } } },
        "definition": { "nodes": [], "edges": [] },
    })
}

fn workflow_run_example() -> Value {
    json!({
        "id": UUID_EXAMPLE,
        "workflow_id": UUID_EXAMPLE,
        "status": "running",
        "name": "manual smoke test",
        "parameters": { "environment": "dev" },
    })
}

fn node_run_example() -> Value {
    json!({
        "id": UUID_EXAMPLE,
        "workflow_run_id": UUID_EXAMPLE,
        "node_id": "deploy",
        "status": "running",
        "attempt": 1,
    })
}

fn trigger_example() -> Value {
    json!({
        "id": UUID_EXAMPLE,
        "workflow_id": UUID_EXAMPLE,
        "enabled": true,
        "kind": "cron",
        "schedule": "0 9 * * *",
    })
}

fn provider_example() -> Value {
    json!({
        "name": "std",
        "version": "1.0.0",
        "actions": [{ "name": "echo", "description": "Return the supplied message." }],
    })
}

fn user_example() -> Value {
    json!({
        "id": UUID_EXAMPLE,
        "username": "admin",
        "email": "admin@example.test",
        "is_admin": true,
        "disabled": false,
    })
}
