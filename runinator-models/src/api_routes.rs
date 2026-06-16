use uuid::Uuid;

pub const API_PROVIDERS: &str = "/providers";
pub const API_AUTH_CONFIG: &str = "/auth/config";
pub const API_AUTH_LOGIN: &str = "/auth/login";
pub const API_AUTH_REFRESH: &str = "/auth/refresh";
pub const API_AUTH_LOGOUT: &str = "/auth/logout";
pub const API_WORKFLOWS: &str = "/workflows";
pub const API_WORKFLOWS_VALIDATE: &str = "/workflows/validate";
pub const API_WORKFLOWS_IMPORT: &str = "/workflows/import";
pub const API_WORKFLOWS_EXPORT: &str = "/workflows/export";
/// compiled pack zip import (workflows + optional secrets).
pub const API_PACKS_IMPORT: &str = "/packs/import";
/// header required before raw json workflow bundle imports are accepted.
pub const WORKFLOW_JSON_IMPORT_RISK_HEADER: &str = "x-runinator-json-workflow-risk";
/// header value acknowledging that raw json workflow imports can break the system.
pub const WORKFLOW_JSON_IMPORT_RISK_ACK: &str = "system-breakage-possible";
pub const API_WDL_COMPLETE: &str = "/wdl/complete";
pub const API_WDL_COMPILE: &str = "/wdl/compile";
pub const API_WDL_ANALYZE: &str = "/wdl/analyze";
pub const API_WDL_FORMAT: &str = "/wdl/format";
pub const API_WDL_DECOMPILE: &str = "/wdl/decompile";
pub const API_WDL_EVALUATE: &str = "/wdl/evaluate";
pub const API_WDL_IMPORT: &str = "/wdl/import";
pub const API_WORKFLOW_TRIGGERS_DUE: &str = "/workflow_triggers/due";
pub const API_SCHEDULER_WORKFLOW_TRIGGER_FIRINGS_CLAIM: &str =
    "/scheduler/workflow_trigger_firings/claim";
pub const API_WORKFLOW_RUNS: &str = "/workflow_runs";
pub const API_SCHEDULER_WORKFLOW_RUNS_CLAIM: &str = "/scheduler/workflow_runs/claim";
pub const API_SCHEDULER_READY_NODES_CLAIM: &str = "/scheduler/ready_nodes/claim";
pub const API_RUNS: &str = "/runs";
pub const API_ARTIFACTS: &str = "/artifacts";
pub const API_SCHEDULER_ACTION_DISPATCHES: &str = "/scheduler/action_dispatches";
pub const API_SCHEDULER_ACTION_DISPATCHES_PENDING: &str = "/scheduler/action_dispatches/pending";
pub const API_SCHEDULER_ACTION_DISPATCHES_CLAIM: &str = "/scheduler/action_dispatches/claim";
pub const API_WORKFLOW_NODE_RUNS: &str = "/workflow_node_runs";
pub const API_SUPERVISOR_STATUS: &str = "/supervisor/status";
pub const API_APPROVALS: &str = "/approvals";
pub const API_IDEMPOTENCY_KEYS: &str = "/idempotency_keys";
pub const API_CREDENTIALS: &str = "/credentials";
pub const API_REPLICAS: &str = "/replicas";

pub fn api_workflow(workflow_id: Uuid) -> String {
    format!("{API_WORKFLOWS}/{workflow_id}")
}

pub fn api_workflow_export(workflow_id: Uuid) -> String {
    format!("{API_WORKFLOWS}/{workflow_id}/export")
}

pub fn api_workflow_duplicate(workflow_id: Uuid) -> String {
    format!("{API_WORKFLOWS}/{workflow_id}/duplicate")
}

pub fn api_workflow_triggers(workflow_id: Uuid) -> String {
    format!("{API_WORKFLOWS}/{workflow_id}/triggers")
}

pub fn api_workflow_runs(workflow_id: Uuid) -> String {
    format!("{API_WORKFLOWS}/{workflow_id}/runs")
}

pub fn api_workflow_trigger(trigger_id: Uuid) -> String {
    format!("/workflow_triggers/{trigger_id}")
}

pub fn api_workflow_trigger_runs(trigger_id: Uuid) -> String {
    format!("/workflow_triggers/{trigger_id}/runs")
}

pub fn api_workflow_run(workflow_run_id: Uuid) -> String {
    format!("{API_WORKFLOW_RUNS}/{workflow_run_id}")
}

pub fn api_workflow_run_rename(workflow_run_id: Uuid) -> String {
    format!("{API_WORKFLOW_RUNS}/{workflow_run_id}/rename")
}

pub fn api_workflow_run_replay(workflow_run_id: Uuid) -> String {
    format!("{API_WORKFLOW_RUNS}/{workflow_run_id}/replay")
}

pub fn api_workflow_run_command(workflow_run_id: Uuid, command: &str) -> String {
    format!("{API_WORKFLOW_RUNS}/{workflow_run_id}/{command}")
}

pub fn api_workflow_run_nodes(workflow_run_id: Uuid) -> String {
    format!("{API_WORKFLOW_RUNS}/{workflow_run_id}/nodes")
}

pub fn api_workflow_run_deliverables(workflow_run_id: Uuid) -> String {
    format!("{API_WORKFLOW_RUNS}/{workflow_run_id}/deliverables")
}

pub fn api_scheduler_workflow_run_claim_renew(workflow_run_id: Uuid) -> String {
    format!("/scheduler/workflow_runs/{workflow_run_id}/claim/renew")
}

pub fn api_scheduler_workflow_run_claim_release(workflow_run_id: Uuid) -> String {
    format!("/scheduler/workflow_runs/{workflow_run_id}/claim/release")
}

pub fn api_scheduler_ready_node_process(ready_node_id: Uuid) -> String {
    format!("/scheduler/ready_nodes/{ready_node_id}/process")
}

pub fn api_scheduler_action_dispatch_published(dispatch_id: Uuid) -> String {
    format!("/scheduler/action_dispatches/{dispatch_id}/published")
}

pub fn api_scheduler_action_dispatch_failed(dispatch_id: Uuid) -> String {
    format!("/scheduler/action_dispatches/{dispatch_id}/failed")
}

pub fn api_run(run_id: Uuid) -> String {
    format!("{API_RUNS}/{run_id}")
}

pub fn api_run_chunks(run_id: Uuid) -> String {
    format!("{API_RUNS}/{run_id}/chunks")
}

pub fn api_run_artifacts(run_id: Uuid) -> String {
    format!("{API_RUNS}/{run_id}/artifacts")
}

pub fn api_workflow_node_run(node_run_id: Uuid) -> String {
    format!("{API_WORKFLOW_NODE_RUNS}/{node_run_id}")
}

pub fn api_workflow_node_run_chunks(node_run_id: Uuid) -> String {
    format!("{API_WORKFLOW_NODE_RUNS}/{node_run_id}/chunks")
}

pub fn api_workflow_node_run_artifacts(node_run_id: Uuid) -> String {
    format!("{API_WORKFLOW_NODE_RUNS}/{node_run_id}/artifacts")
}

pub fn api_artifact_download(artifact_id: Uuid) -> String {
    format!("{API_ARTIFACTS}/{artifact_id}/download")
}

pub fn api_workflow_node_run_claim(node_run_id: Uuid) -> String {
    format!("{API_WORKFLOW_NODE_RUNS}/{node_run_id}/claim")
}

pub fn api_workflow_node_run_release(node_run_id: Uuid) -> String {
    format!("{API_WORKFLOW_NODE_RUNS}/{node_run_id}/release")
}

pub fn api_approval_command(approval_id: Uuid, command: &str) -> String {
    format!("{API_APPROVALS}/{approval_id}/{command}")
}

pub fn api_replica(replica_id: Uuid) -> String {
    format!("{API_REPLICAS}/{replica_id}")
}

pub fn api_replica_heartbeat(replica_id: Uuid) -> String {
    format!("{API_REPLICAS}/{replica_id}/heartbeat")
}

pub fn api_replica_offline(replica_id: Uuid) -> String {
    format!("{API_REPLICAS}/{replica_id}/offline")
}

pub fn api_replica_providers(replica_id: Uuid) -> String {
    format!("{API_REPLICAS}/{replica_id}/providers")
}
