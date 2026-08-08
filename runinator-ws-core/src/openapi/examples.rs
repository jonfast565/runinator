//! response and request examples referenced by [`super::docs::EndpointDoc`].
//!
//! kept apart from the endpoint table so a new example is one enum arm and one match arm rather
//! than an edit inside two thousand lines of endpoint entries.

use serde_json::{Value, json};

/// the single uuid used across every example so generated samples look internally consistent.
pub const UUID_EXAMPLE: &str = "018f5f7c-4b74-7f44-8fd1-cde6b5c4d111";

#[derive(Clone, Copy)]
pub enum Example {
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
    EventDelivery,
    Supervisor,
}

pub fn example_value(example: Example) -> Option<Value> {
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
        Example::EventDelivery => {
            json!({ "type": "deploy.finished", "data": { "environment": "prod", "revision": "abc123" } })
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
