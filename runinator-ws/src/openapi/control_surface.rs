//! operator capability ownership for the generated API surface.

use runinator_ws_core::openapi::docs::EndpointPolicy;
use serde_json::{Map, Value, json};

const CAPABILITY_PATHS: &[(&str, &str)] = &[
    ("/orchestrations/adapters", "adapters"),
    ("/ingress_control", "ingress-control"),
    ("/ingress/admission", "ingress-control"),
    ("/execution_profiles", "execution-profiles"),
    ("/notification_policies", "notifications"),
    ("/notifications", "notifications"),
    ("/automation_events", "external-operations"),
    ("/external_items", "external-operations"),
    ("/idempotency_keys", "external-operations"),
    ("/approvals", "external-operations"),
    ("/gates", "external-operations"),
    ("/server/settings", "operational-policy"),
    ("/auth/settings", "operational-policy"),
    ("/worker/settings", "operational-policy"),
    ("/freeze_windows", "operational-policy"),
    ("/schedules", "operational-policy"),
    ("/calendar", "operational-policy"),
    ("/providers", "providers"),
    ("/orchestrations", "orchestrations"),
    ("/replicas", "runtime-fleet"),
    ("/agents", "runtime-fleet"),
    ("/nodes", "runtime-fleet"),
    ("/supervisor", "runtime-fleet"),
    ("/workflow_runs", "workflow-runtime"),
    ("/workflow_effects", "workflow-runtime"),
    ("/workflow_continuations", "workflow-runtime"),
    ("/workflow_triggers", "workflow-authoring"),
    ("/workflows", "workflow-authoring"),
    ("/rexrap", "workflow-authoring"),
    ("/node-kinds", "workflow-authoring"),
    ("/trigger-kinds", "workflow-authoring"),
    ("/catalog", "workflow-authoring"),
    ("/pipeline_runs", "pipelines"),
    ("/pipeline_triggers", "pipelines"),
    ("/pipelines", "pipelines"),
    ("/function_invocations", "functions"),
    ("/function_packages", "functions"),
    ("/function_exports", "functions"),
    ("/function_artifacts", "functions"),
    ("/functions", "functions"),
    ("/workspace-", "workspaces"),
    ("/workspaces", "workspaces"),
    ("/workflow_files", "workflow-files"),
    ("/artifacts", "workflow-files"),
    ("/console", "console"),
    ("/packs", "packs"),
    ("/audit_log", "observability"),
    ("/dead_letters", "observability"),
    ("/broker_messages", "observability"),
    ("/auth/switch", "organizations"),
    ("/orgs", "organizations"),
    ("/rate-card", "organizations"),
    ("/authz", "access-control"),
    ("/service_accounts", "access-control"),
    ("/api_keys", "access-control"),
    ("/users", "access-control"),
    ("/teams", "access-control"),
    ("/auth", "access-control"),
    ("/credentials", "credentials"),
    ("/ws/events", "observability"),
    ("/ws/workflow-runs", "workflow-runtime"),
];

pub(super) fn enrich_control_surface(
    operation: &mut Map<String, Value>,
    path: &str,
    policy: EndpointPolicy,
) {
    let surface = endpoint_control_surface(path, policy);
    operation.insert("x-runinator-control-surface".into(), json!(surface));

    if surface == "operator" {
        if let Some(capability) = operator_capability(path) {
            operation.insert("x-runinator-capability".into(), json!(capability));
        }
        return;
    }

    operation.insert(
        "x-runinator-control-surface-reason".into(),
        json!(control_surface_reason(surface)),
    );
}

pub(super) fn endpoint_control_surface(path: &str, policy: EndpointPolicy) -> &'static str {
    if matches!(policy, EndpointPolicy::SystemRole(_)) {
        return "headless";
    }
    if policy.is_public() {
        return if matches!(
            path,
            "/auth/config" | "/auth/login" | "/auth/refresh" | "/agents/enroll"
        ) {
            "bootstrap"
        } else {
            "data_plane"
        };
    }
    "operator"
}

pub(super) fn operator_capability(path: &str) -> Option<&'static str> {
    CAPABILITY_PATHS
        .iter()
        .find_map(|(prefix, capability)| path.starts_with(prefix).then_some(*capability))
}

fn control_surface_reason(surface: &str) -> &'static str {
    match surface {
        "headless" => "Machine-to-machine protocol authenticated by a fixed system role.",
        "bootstrap" => "Required before an authenticated Command Center session exists.",
        _ => "Public data-plane, callback, health, documentation, or capability URL.",
    }
}
