#[allow(unused_imports)]
use super::*;

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
        (name = "Settings", description = "Platform-wide server operating policy."),
        (name = "Debug", description = "Workflow-run debugger commands."),
        (name = "Notifications", description = "User notification records."),
        (name = "Providers", description = "Registered task providers."),
        (name = "Functions", description = "Packaged functions: publishing, promotion, and artifacts."),
        (name = "Orchestrations", description = "Durable correlated execution state, history, and intent controls."),
        (name = "Orchestration Adapters", description = "Verified external-event adapters and their plugin catalog."),
        (name = "Console", description = "The REXRAP console: notebook sessions and their cells."),
        (name = "Replicas", description = "Service replica registry."),
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
        crate::handlers::observability::get_broker_messages,
        crate::handlers::observability::get_audit_log,
        crate::handlers::observability::post_runtime_logs,
        crate::handlers::observability::get_runtime_logs,
    ),
    components(schemas(ApiError)),
)]
pub struct ApiDoc;
