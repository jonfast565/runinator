pub const API_PROVIDERS: &str = "/providers";
pub const API_WORKFLOWS: &str = "/workflows";
pub const API_WORKFLOWS_VALIDATE: &str = "/workflows/validate";
pub const API_WORKFLOWS_IMPORT: &str = "/workflows/import";
pub const API_WORKFLOWS_EXPORT: &str = "/workflows/export";
pub const API_WORKFLOW_TRIGGERS_DUE: &str = "/workflow_triggers/due";
pub const API_SCHEDULER_WORKFLOW_TRIGGER_FIRINGS_CLAIM: &str =
    "/scheduler/workflow_trigger_firings/claim";
pub const API_WORKFLOW_RUNS: &str = "/workflow_runs";
pub const API_SCHEDULER_WORKFLOW_RUNS_CLAIM: &str = "/scheduler/workflow_runs/claim";
pub const API_RUNS: &str = "/runs";
pub const API_ARTIFACTS: &str = "/artifacts";
pub const API_SCHEDULER_ACTION_DISPATCHES: &str = "/scheduler/action_dispatches";
pub const API_SCHEDULER_ACTION_DISPATCHES_PENDING: &str = "/scheduler/action_dispatches/pending";
pub const API_WORKFLOW_NODE_RUNS: &str = "/workflow_node_runs";
pub const API_SUPERVISOR_STATUS: &str = "/supervisor/status";
pub const API_APPROVALS: &str = "/approvals";
pub const API_IDEMPOTENCY_KEYS: &str = "/idempotency_keys";
pub const API_CREDENTIALS: &str = "/credentials";

pub fn api_workflow(workflow_id: i64) -> String {
    format!("{API_WORKFLOWS}/{workflow_id}")
}

pub fn api_workflow_export(workflow_id: i64) -> String {
    format!("{API_WORKFLOWS}/{workflow_id}/export")
}

pub fn api_workflow_triggers(workflow_id: i64) -> String {
    format!("{API_WORKFLOWS}/{workflow_id}/triggers")
}

pub fn api_workflow_runs(workflow_id: i64) -> String {
    format!("{API_WORKFLOWS}/{workflow_id}/runs")
}

pub fn api_workflow_trigger(trigger_id: i64) -> String {
    format!("/workflow_triggers/{trigger_id}")
}

pub fn api_workflow_trigger_runs(trigger_id: i64) -> String {
    format!("/workflow_triggers/{trigger_id}/runs")
}

pub fn api_workflow_run(workflow_run_id: i64) -> String {
    format!("{API_WORKFLOW_RUNS}/{workflow_run_id}")
}

pub fn api_workflow_run_rename(workflow_run_id: i64) -> String {
    format!("{API_WORKFLOW_RUNS}/{workflow_run_id}/rename")
}

pub fn api_workflow_run_replay(workflow_run_id: i64) -> String {
    format!("{API_WORKFLOW_RUNS}/{workflow_run_id}/replay")
}

pub fn api_workflow_run_command(workflow_run_id: i64, command: &str) -> String {
    format!("{API_WORKFLOW_RUNS}/{workflow_run_id}/{command}")
}

pub fn api_workflow_run_nodes(workflow_run_id: i64) -> String {
    format!("{API_WORKFLOW_RUNS}/{workflow_run_id}/nodes")
}

pub fn api_scheduler_workflow_run_claim_renew(workflow_run_id: i64) -> String {
    format!("/scheduler/workflow_runs/{workflow_run_id}/claim/renew")
}

pub fn api_scheduler_workflow_run_claim_release(workflow_run_id: i64) -> String {
    format!("/scheduler/workflow_runs/{workflow_run_id}/claim/release")
}

pub fn api_scheduler_action_dispatch_published(dispatch_id: i64) -> String {
    format!("/scheduler/action_dispatches/{dispatch_id}/published")
}

pub fn api_scheduler_action_dispatch_failed(dispatch_id: i64) -> String {
    format!("/scheduler/action_dispatches/{dispatch_id}/failed")
}

pub fn api_run(run_id: i64) -> String {
    format!("{API_RUNS}/{run_id}")
}

pub fn api_run_chunks(run_id: i64) -> String {
    format!("{API_RUNS}/{run_id}/chunks")
}

pub fn api_run_artifacts(run_id: i64) -> String {
    format!("{API_RUNS}/{run_id}/artifacts")
}

pub fn api_workflow_node_run(node_run_id: i64) -> String {
    format!("{API_WORKFLOW_NODE_RUNS}/{node_run_id}")
}

pub fn api_workflow_node_run_chunks(node_run_id: i64) -> String {
    format!("{API_WORKFLOW_NODE_RUNS}/{node_run_id}/chunks")
}

pub fn api_workflow_node_run_artifacts(node_run_id: i64) -> String {
    format!("{API_WORKFLOW_NODE_RUNS}/{node_run_id}/artifacts")
}

pub fn api_approval_command(approval_id: i64, command: &str) -> String {
    format!("{API_APPROVALS}/{approval_id}/{command}")
}
