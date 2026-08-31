//! response and request examples referenced by [`super::docs::EndpointDoc`].
//!
//! kept apart from the endpoint table so a new example is one enum arm and one match arm rather
//! than an edit inside two thousand lines of endpoint entries.

use serde_json::{Value, json};

/// the single UUID used across every example so generated samples look internally consistent.
pub const UUID_EXAMPLE: &str = "018f5f7c-4b74-7f44-8fd1-cde6b5c4d111";
const FUNCTION_DIGEST_EXAMPLE: &str =
    "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
const TIMESTAMP_EXAMPLE: &str = "2026-01-01T00:00:00Z";

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
    IngressEvent,
    IngressResponse,
    IngressAdmission,
    IngressTimeline,
    Artifact,
    RexRapSource,
    RexRapCompile,
    RexRapCompletion,
    RexRapHover,
    RexRapDiagnostics,
    RexRapDecompile,
    RexRapEvaluate,
    Trigger,
    TriggerList,
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
    AgentEnrollmentCreate,
    AgentEnrollmentTokenList,
    AgentEnrollmentRequest,
    AgentEnrollmentResponse,
    AgentDirective,
    AgentDirectiveList,
    Grant,
    Team,
    WebhookSignal,
    EventDelivery,
    InterruptRequest,
    Supervisor,
    FunctionPackage,
    FunctionPackageList,
    FunctionVersion,
    FunctionCatalog,
    FunctionArtifact,
    FunctionAlias,
    FunctionAliasRequest,
    FunctionPublish,
    FunctionInvocationTarget,
    OrchestrationBinding,
    OrchestrationBindingList,
    OrchestrationCorrelationAlias,
    OrchestrationCorrelationAliasList,
    OrchestrationCorrelationAliasRequest,
    OrchestrationEpochList,
    OrchestrationReductionList,
    OrchestrationEvidenceList,
    OrchestrationCommandList,
    OrchestrationIntentRequest,
    OrchestrationRequeueRequest,
    ExternalOperation,
    ExternalOperationList,
    ExternalOperationResolution,
    WorkspaceList,
    AdapterKindList,
    AdapterDefinition,
    AdapterDefinitionList,
    AdapterRevisionList,
    AdapterPollStatus,
    AdapterApply,
    AdapterEnable,
    AdapterTest,
    AdapterTestResult,
    AdapterHealth,
    AdapterWebhookResponse,
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
        Example::IngressEvent => json!({
            "source": "generic-webhook", "event_id": "delivery-123", "event_type": "updated",
            "correlation_key": "release-42", "payload": { "revision": "abc123" },
            "occurred_at": TIMESTAMP_EXAMPLE
        }),
        Example::IngressResponse => json!({
            "admission_id": UUID_EXAMPLE, "generation": 2, "disposition": "queued",
            "duplicate": false, "queue_position": 1, "workflow_run_id": null,
            "pipeline_run_id": null, "message": "ingress event queued"
        }),
        Example::IngressAdmission => json!({
            "id": UUID_EXAMPLE, "org_id": null, "scope": "release.lifecycle",
            "correlation_key": "release-42", "generation": 2,
            "target": { "kind": "workflow", "id": UUID_EXAMPLE }, "status": "active",
            "workflow_run_id": UUID_EXAMPLE, "pipeline_run_id": null,
            "policy": { "scope": "release.lifecycle", "routes": [
                { "event_type": "created", "lifecycle": "unbound", "action": "start" },
                { "event_type": "updated", "lifecycle": "active", "action": "queue" },
                { "event_type": "canceled", "lifecycle": "active", "action": "interrupt" },
                { "event_type": "observed", "lifecycle": "active", "action": "record" },
                { "event_type": "reopened", "lifecycle": "terminal", "action": "requeue" }
            ]}, "created_at": TIMESTAMP_EXAMPLE, "updated_at": TIMESTAMP_EXAMPLE
        }),
        Example::IngressTimeline => json!([{
            "id": UUID_EXAMPLE, "admission_id": UUID_EXAMPLE, "sequence": 1, "generation": 1,
            "source": "generic-webhook", "event_id": "delivery-123", "event_type": "created",
            "correlation_key": "release-42", "payload": {}, "occurred_at": null,
            "received_at": TIMESTAMP_EXAMPLE, "disposition": "started", "queue_state": "none",
            "queue_position": null, "promoted_generation": null,
            "workflow_run_id": UUID_EXAMPLE, "pipeline_run_id": null
        }]),
        Example::Artifact => {
            json!({ "id": UUID_EXAMPLE, "name": "report.json", "content_type": "application/json", "size": 42 })
        }
        Example::RexRapSource => {
            json!({ "source": "workflow hello {\n  task echo uses std.echo\n}\n", "fragment": "expression" })
        }
        Example::RexRapCompile => {
            json!({ "source": "workflow hello {\n  task echo uses std.echo\n}\n", "enabled": true })
        }
        Example::RexRapCompletion => {
            json!({ "source": "workflow hello {\n  ", "cursor_byte": 19, "providers": [], "settings": [] })
        }
        Example::RexRapHover => {
            json!({ "range_start_byte": 18, "range_end_byte": 24, "title": "params", "kind": "parameter root", "detail": "{ name: string }", "documentation": "Workflow input parameters." })
        }
        Example::RexRapDiagnostics => {
            json!([{ "start": 0, "end": 4, "line": 1, "column": 1, "severity": "warning", "message": "example diagnostic" }])
        }
        Example::RexRapDecompile => json!({ "workflow": workflow_example() }),
        Example::RexRapEvaluate => {
            json!({ "source": "inputs.environment == \"prod\"", "kind": "condition", "context": { "inputs": { "environment": "dev" } } })
        }
        Example::Trigger => trigger_example(),
        Example::TriggerList => json!([trigger_example()]),
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
        Example::Provider => provider_example(),
        Example::ProviderList => json!([provider_example()]),
        Example::FunctionPackage => function_package_example(),
        Example::FunctionPackageList => json!([function_package_example()]),
        Example::FunctionVersion => function_version_example(),
        Example::FunctionCatalog => json!([function_catalog_example()]),
        Example::FunctionArtifact => json!({
            "digest": FUNCTION_DIGEST_EXAMPLE,
            "size_bytes": 20480,
            "uri": "blob://runinator-function-artifacts/sha256/9f/86/9f86...zip",
            "media_type": "application/zip",
            "created_at": TIMESTAMP_EXAMPLE,
        }),
        Example::FunctionAlias => json!({
            "id": UUID_EXAMPLE,
            "package_id": UUID_EXAMPLE,
            "name": "production",
            "version_id": UUID_EXAMPLE,
            "version": 3,
        }),
        Example::FunctionInvocationTarget => json!({
            "package_name": "image-tools",
            "version": 3,
            "artifact_digest": FUNCTION_DIGEST_EXAMPLE,
            "runtime": { "runtime": "python3.13" },
            "export": {
                "id": UUID_EXAMPLE,
                "version_id": UUID_EXAMPLE,
                "name": "resize",
                "handler": "src.images.resize",
                "limits": { "timeout_seconds": 60, "memory_mb": 512 },
            },
        }),
        Example::FunctionAliasRequest => json!({ "alias": "production", "version": 3 }),
        Example::FunctionPublish => json!({
            "package": { "name": "image-tools", "description": "image utilities" },
            "artifact_digest": FUNCTION_DIGEST_EXAMPLE,
            "runtime": { "runtime": "python3.13" },
            "exports": [{
                "name": "resize",
                "handler": "src.images.resize",
                "input": [{ "name": "source", "type": "string", "required": true }],
                "output": [{ "name": "uri", "type": "string" }],
            }],
            "alias": "latest",
        }),
        Example::OrchestrationBinding => orchestration_binding_example(),
        Example::OrchestrationBindingList => json!([orchestration_binding_example()]),
        Example::OrchestrationCorrelationAlias => orchestration_correlation_alias_example(),
        Example::OrchestrationCorrelationAliasList => {
            json!([orchestration_correlation_alias_example()])
        }
        Example::OrchestrationCorrelationAliasRequest => json!({
            "source": "github",
            "scope": "pull-requests",
            "correlation_key": "octo/repo#42"
        }),
        Example::OrchestrationEpochList => json!([{
            "id": UUID_EXAMPLE, "binding_id": UUID_EXAMPLE, "epoch": 2,
            "pipeline_run_id": UUID_EXAMPLE, "start_member": "implementation",
            "parameters": { "subject_revision": "abc123" }, "status": "running",
            "reason": "scope_changed", "created_at": TIMESTAMP_EXAMPLE,
            "started_at": TIMESTAMP_EXAMPLE, "finished_at": null
        }]),
        Example::OrchestrationReductionList => json!([{
            "id": UUID_EXAMPLE, "binding_id": UUID_EXAMPLE, "inbox_event_id": UUID_EXAMPLE,
            "sequence": 7, "matched_intents": ["rework", "observe"], "winner": "rework",
            "suppressed_intents": ["observe"], "binding_version": 12,
            "disposition": "superseded", "detail": { "event": { "event_type": "updated" } },
            "created_at": TIMESTAMP_EXAMPLE
        }]),
        Example::OrchestrationEvidenceList => json!([{
            "id": UUID_EXAMPLE, "binding_id": UUID_EXAMPLE, "epoch": 2,
            "kind": "verification", "subject_revision": "abc123",
            "payload": { "checks": ["lint", "test"], "passed": true },
            "source_event_id": null, "created_at": TIMESTAMP_EXAMPLE
        }]),
        Example::OrchestrationCommandList => json!([{
            "id": UUID_EXAMPLE, "binding_id": UUID_EXAMPLE, "epoch": 2,
            "command_type": "start_epoch", "operation_key": "binding:epoch:2:start",
            "payload": {}, "status": "succeeded", "attempts": 1,
            "claimed_by": null, "claimed_until": null, "result": { "pipeline_run_id": UUID_EXAMPLE },
            "created_at": TIMESTAMP_EXAMPLE, "updated_at": TIMESTAMP_EXAMPLE
        }]),
        Example::OrchestrationIntentRequest => json!({
            "intent": "pause", "payload": { "note": "operator requested" },
            "reason": "waiting for approval", "idempotency_key": "pause-2026-01-01"
        }),
        Example::OrchestrationRequeueRequest => json!({
            "reason": "input corrected", "idempotency_key": "requeue-2026-01-01"
        }),
        Example::ExternalOperation => external_operation_example(),
        Example::ExternalOperationList => json!([external_operation_example()]),
        Example::ExternalOperationResolution => json!({
            "resolution": "succeeded", "reason": "verified in provider console",
            "receipt": { "external_id": "operation-42" }
        }),
        Example::WorkspaceList => json!([{
            "id": UUID_EXAMPLE, "admission_id": UUID_EXAMPLE, "generation": 1,
            "scope": "source", "attempt": 2, "worker_instance_id": "worker-1",
            "local_key": "workspace-42", "requirements": { "capability": "git" },
            "status": "active", "version": 3, "leased_until": TIMESTAMP_EXAMPLE,
            "evidence": {}, "created_at": TIMESTAMP_EXAMPLE, "updated_at": TIMESTAMP_EXAMPLE
        }]),
        Example::AdapterKindList => json!([{
            "metadata": {
                "kind": "generic-webhook", "version": "1", "display_name": "Generic webhook",
                "fields": [{ "name": "delivery_id_pointer", "value_type": "string", "required": true, "secret": false, "default": null }],
                "event_names": ["created", "updated"], "canonical_pointers": ["/subject/id"],
                "capabilities": ["hmac_sha256", "bearer"]
            },
            "origin": "builtin", "healthy": true, "error": null
        }]),
        Example::AdapterDefinition => adapter_definition_example(),
        Example::AdapterDefinitionList => json!([adapter_definition_example()]),
        Example::AdapterRevisionList => json!([{
            "id": UUID_EXAMPLE, "adapter_id": UUID_EXAMPLE, "revision": 1,
            "kind_version": "1", "transport": "webhook", "configuration": { "delivery_id_pointer": "/delivery_id" },
            "secret_bindings": { "signing_secret": UUID_EXAMPLE },
            "identity_configuration": {}, "created_at": TIMESTAMP_EXAMPLE, "actor_id": UUID_EXAMPLE
        }]),
        Example::AdapterPollStatus => json!({
            "adapter_id": UUID_EXAMPLE, "revision": 2,
            "checkpoint": { "updated_at": TIMESTAMP_EXAMPLE },
            "next_poll_at": TIMESTAMP_EXAMPLE, "claimed_until": null,
            "last_attempt_at": TIMESTAMP_EXAMPLE, "last_success_at": TIMESTAMP_EXAMPLE,
            "last_error": null
        }),
        Example::AdapterApply => json!({
            "name": "work item events", "kind": "generic-webhook", "kind_version": "1",
            "transport": "webhook",
            "configuration": {
                "delivery_id_pointer": "/delivery_id", "scope_pointer": "/scope",
                "correlation_pointer": "/subject/id", "event_pointer": "/event"
            },
            "secret_bindings": { "signing_secret": UUID_EXAMPLE },
            "identity_configuration": {}, "expected_revision": 1
        }),
        Example::AdapterEnable => json!({ "enabled": true }),
        Example::AdapterTest => json!({
            "headers": { "x-delivery-id": "delivery-123" },
            "body_base64": "eyJldmVudCI6InVwZGF0ZWQifQ=="
        }),
        Example::AdapterTestResult => json!({
            "verified": true,
            "events": [{
                "source": "generic-webhook", "delivery_id": "delivery-123",
                "event_type": "updated", "scope": "work-items", "correlation_key": "item-42",
                "subject_revision": "abc123", "payload": {}, "provenance": {}
            }],
            "errors": [], "previews": [{ "candidate_intents": ["rework"], "winner": "rework" }]
        }),
        Example::AdapterHealth => json!({ "status": "healthy", "loaded_kinds": 3, "errors": [] }),
        Example::AdapterWebhookResponse => json!({
            "adapter_id": UUID_EXAMPLE, "adapter_revision": 1,
            "outcomes": [{ "disposition": "recorded", "duplicate": false }]
        }),
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
            json!({ "name": "local automation", "principal_kind": "user", "principal_id": UUID_EXAMPLE, "system_role": null, "action_ceiling": [], "expires_at": null })
        }
        Example::ApiKeyList => {
            json!([{ "id": UUID_EXAMPLE, "name": "local automation", "principal_kind": "user", "principal_id": UUID_EXAMPLE, "system_role": null, "action_ceiling": [], "key_prefix": "runi_live_1234", "expires_at": null, "disabled": false }])
        }
        Example::AgentEnrollmentCreate => json!({
            "ttl_seconds": 900,
            "org_id": UUID_EXAMPLE,
            "labels": { "site": "home" },
            "service_url": "https://runinator.example",
            "permanent": true
        }),
        Example::AgentEnrollmentTokenList => json!([{
            "token_id": "AbCdEfGhIjk",
            "org_id": UUID_EXAMPLE,
            "labels": { "site": "home" },
            "service_url": "https://runinator.example",
            "permanent": true,
            "expires_at": "2026-08-12T16:15:00Z",
            "consumed_at": null,
            "created_at": "2026-08-12T16:00:00Z"
        }]),
        Example::AgentEnrollmentRequest => json!({
            "token_id": "AbCdEfGhIjk",
            "request_body": {
                "instance_id": "home-agent-01",
                "display_name": "Home server",
                "labels": { "site": "home" }
            },
            "proof": "base64url-hmac-sha256"
        }),
        Example::AgentEnrollmentResponse => json!({
            "api_key": "runi_agent_secret_shown_once",
            "service_url": "https://runinator.example",
            "expires_at": null,
            "org_id": UUID_EXAMPLE,
            "labels": { "site": "home" }
        }),
        Example::AgentDirective => agent_directive_example(),
        Example::AgentDirectiveList => json!([agent_directive_example()]),
        Example::Grant => {
            json!({ "principal_type": "user", "principal_id": UUID_EXAMPLE, "permission": "view" })
        }
        Example::Team => json!({ "name": "platform", "user_id": UUID_EXAMPLE }),
        Example::WebhookSignal => {
            json!({ "name": "ticket.closed", "correlation_key": "PROJ-123", "payload": { "status": "done" } })
        }
        Example::EventDelivery => {
            json!({ "type": "deploy.finished", "data": { "environment": "prod", "revision": "abc123" } })
        }
        Example::InterruptRequest => {
            json!({ "source": "external", "payload": { "reason": "credentials rotated" } })
        }
        Example::Supervisor => {
            json!({ "running": true, "services": [{ "name": "runinator-ws", "status": "running" }] })
        }
    })
}

fn agent_directive_example() -> Value {
    json!({
        "directive_id": UUID_EXAMPLE,
        "replica_id": UUID_EXAMPLE,
        "kind": { "type": "diagnostics" },
        "state": "pending",
        "issued_at": "2026-08-12T12:00:00Z",
        "expires_at": "2026-08-12T12:05:00Z",
        "payload": null,
        "attempts": 0
    })
}

fn workflow_example() -> Value {
    json!({
        "id": UUID_EXAMPLE,
        "name": "hello-world",
        "namespace": "default",
        "version": "1.0.0",
        "enabled": true,
        "input_type": { "type": "object", "properties": { "environment": { "type": "string" } } },
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

fn function_package_example() -> Value {
    json!({
        "id": UUID_EXAMPLE,
        "name": "image-tools",
        "description": "image utilities",
        "latest_version": 3,
        "created_at": TIMESTAMP_EXAMPLE,
        "updated_at": TIMESTAMP_EXAMPLE,
    })
}

fn function_version_example() -> Value {
    json!({
        "id": UUID_EXAMPLE,
        "package_id": UUID_EXAMPLE,
        "version": 3,
        "artifact_digest": FUNCTION_DIGEST_EXAMPLE,
        "runtime": { "runtime": "python3.13" },
        "created_at": TIMESTAMP_EXAMPLE,
    })
}

fn function_catalog_example() -> Value {
    json!({
        "package_id": UUID_EXAMPLE,
        "package_name": "image-tools",
        "version_id": UUID_EXAMPLE,
        "version": 3,
        "export_id": UUID_EXAMPLE,
        "export_name": "resize",
        "artifact_digest": FUNCTION_DIGEST_EXAMPLE,
        "input": [{ "name": "source", "ty": "string", "required": true }],
        "output": [{ "name": "uri", "ty": "string" }],
        "aliases": ["latest", "production"],
    })
}

fn provider_example() -> Value {
    json!({
        "name": "std",
        "version": "1.0.0",
        "actions": [{ "name": "echo", "description": "Return the supplied message." }],
    })
}

fn orchestration_binding_example() -> Value {
    json!({
        "id": UUID_EXAMPLE, "admission_id": UUID_EXAMPLE, "org_id": UUID_EXAMPLE,
        "scope": "work-items", "correlation_key": "item-42", "generation": 1,
        "pipeline_id": UUID_EXAMPLE, "pipeline_revision": 4, "pipeline_digest": "sha256:abc123",
        "adapter_id": UUID_EXAMPLE, "adapter_revision": 1,
        "policy": { "intents": {}, "phases": {}, "budgets": {}, "defaults": null },
        "status": "running", "current_phase": "implementation", "current_attempt": 2,
        "current_epoch": 2, "restart_member": null, "resume_existing_epoch": false,
        "subject_revision": "abc123", "resources": {}, "budgets": { "transient": 1 },
        "last_reduced_sequence": 7, "version": 12, "reducer_lease_owner": null,
        "reducer_leased_until": null, "created_at": TIMESTAMP_EXAMPLE,
        "updated_at": TIMESTAMP_EXAMPLE, "finished_at": null
    })
}

fn orchestration_correlation_alias_example() -> Value {
    json!({
        "id": UUID_EXAMPLE,
        "binding_id": UUID_EXAMPLE,
        "generation": 1,
        "org_id": UUID_EXAMPLE,
        "source": "github",
        "scope": "pull-requests",
        "correlation_key": "octo/repo#42",
        "created_at": TIMESTAMP_EXAMPLE,
        "updated_at": TIMESTAMP_EXAMPLE
    })
}

fn external_operation_example() -> Value {
    json!({
        "id": UUID_EXAMPLE, "binding_id": UUID_EXAMPLE, "epoch": 2,
        "workflow_run_id": UUID_EXAMPLE, "effect_id": UUID_EXAMPLE,
        "operation_key": "ensure-pr:item-42", "provider": "github", "action": "ensure_pr",
        "semantics": "reconcilable", "attempt": 1, "status": "waiting", "ambiguous": true,
        "provenance": { "operation_key": "ensure-pr:item-42" }, "receipt": {},
        "created_at": TIMESTAMP_EXAMPLE, "updated_at": TIMESTAMP_EXAMPLE
    })
}

fn adapter_definition_example() -> Value {
    json!({
        "id": UUID_EXAMPLE, "org_id": UUID_EXAMPLE, "name": "work item events",
        "kind": "generic-webhook", "current_revision": 1, "enabled": true,
        "endpoint_identity": "c0f83a4a-a8b9-45d0-a3ac-811c729ab421",
        "has_admitted_binding": false, "created_at": TIMESTAMP_EXAMPLE,
        "updated_at": TIMESTAMP_EXAMPLE
    })
}

fn user_example() -> Value {
    json!({
        "id": UUID_EXAMPLE,
        "username": "admin",
        "email": "admin@example.test",
        "platform_role": "admin",
        "disabled": false,
    })
}
