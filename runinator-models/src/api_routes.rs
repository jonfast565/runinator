use uuid::Uuid;

pub const API_PROVIDERS: &str = "/providers";
pub const API_AUTH_CONFIG: &str = "/auth/config";
pub const API_AUTH_LOGIN: &str = "/auth/login";
pub const API_AUTH_REFRESH: &str = "/auth/refresh";
pub const API_AUTH_LOGOUT: &str = "/auth/logout";
pub const API_AUTH_SWITCH_ORG: &str = "/auth/switch-org";
pub const API_AUTH_SWITCH_PLATFORM: &str = "/auth/switch-platform";
pub const API_WORKFLOWS: &str = "/workflows";
pub const API_WORKFLOWS_VALIDATE: &str = "/workflows/validate";
/// server-side dry-run / branch preview (no actions published).
pub const API_WORKFLOWS_SIMULATE: &str = "/workflows/simulate";
pub const API_WORKFLOWS_EXPORT: &str = "/workflows/export";
/// compiled pack zip import (workflows + optional secrets).
pub const API_PACKS_IMPORT: &str = "/packs/import";
pub const API_REXRAP_COMPLETE: &str = "/rexrap/complete";
pub const API_REXRAP_HOVER: &str = "/rexrap/hover";
pub const API_REXRAP_COMPILE: &str = "/rexrap/compile";
pub const API_REXRAP_ANALYZE: &str = "/rexrap/analyze";
pub const API_REXRAP_FORMAT: &str = "/rexrap/format";
pub const API_REXRAP_DECOMPILE: &str = "/rexrap/decompile";
pub const API_REXRAP_DECOMPILE_SPANS: &str = "/rexrap/decompile/spans";
pub const API_REXRAP_EVALUATE: &str = "/rexrap/evaluate";
pub const API_REXRAP_IMPORT: &str = "/rexrap/import";
pub const API_WORKFLOW_TRIGGERS_DUE: &str = "/workflow_triggers/due";
pub const API_FREEZE_WINDOWS: &str = "/freeze_windows";
pub const API_SCHEDULE_CALENDAR: &str = "/schedules/calendar.ics";
pub const API_CALENDAR_SUBSCRIPTIONS: &str = "/schedules/calendar-subscriptions";
pub const API_PIPELINES: &str = "/pipelines";
/// packaged function packages: list and publish.
pub const API_FUNCTIONS: &str = "/functions";
/// the flattened catalog of every published export, which is what a compile types calls against.
pub const API_FUNCTIONS_CATALOG: &str = "/functions/catalog";
/// resolve one export to what a worker needs to run it.
pub const API_FUNCTION_EXPORTS: &str = "/function_exports";
/// content-addressed package archives, keyed by `sha256:<hex>`.
pub const API_FUNCTION_ARTIFACTS: &str = "/function_artifacts";
pub const API_WORKFLOW_RUNS: &str = "/workflow_runs";
/// Compiled-VM execution records. These replace node-run history for VM-backed runs.
pub const API_WORKFLOW_CONTINUATIONS: &str = "/workflow_continuations";
pub const API_WORKFLOW_EFFECTS: &str = "/workflow_effects";
pub const API_SCHEDULER_WORKFLOW_RUNS_CLAIM: &str = "/scheduler/workflow_runs/claim";
/// Store artifact bytes and return the URI to record.
/// This creates no row; the caller already accounted for the artifact.
pub const API_ARTIFACTS_CONTENT: &str = "/artifacts/content";
/// VM-native user-uploaded workflow inputs and reusable library revisions.
pub const API_WORKFLOW_FILES: &str = "/workflow_files";
pub const API_SUPERVISOR_STATUS: &str = "/supervisor/status";
pub const API_APPROVALS: &str = "/approvals";
pub const API_IDEMPOTENCY_KEYS: &str = "/idempotency_keys";
/// reserve an action node's idempotency key before its provider is invoked.
pub const API_IDEMPOTENCY_KEYS_CLAIM: &str = "/idempotency_keys/claim";
/// record a completed execution against a reserved key so a redelivery replays it.
pub const API_IDEMPOTENCY_KEYS_COMPLETE: &str = "/idempotency_keys/complete";
/// free an unfinished reservation after a non-success outcome.
pub const API_IDEMPOTENCY_KEYS_RELEASE: &str = "/idempotency_keys/release";
pub const API_CREDENTIALS: &str = "/credentials";
pub const API_EXECUTION_PROFILES: &str = "/execution_profiles";
pub const API_REPLICAS: &str = "/replicas";
pub const API_ORCHESTRATIONS: &str = "/orchestrations";

pub fn api_workflow(workflow_id: Uuid) -> String {
    format!("{API_WORKFLOWS}/{workflow_id}")
}

pub fn api_workflow_export(workflow_id: Uuid) -> String {
    format!("{API_WORKFLOWS}/{workflow_id}/export")
}

pub fn api_workflow_duplicate(workflow_id: Uuid) -> String {
    format!("{API_WORKFLOWS}/{workflow_id}/duplicate")
}

pub fn api_workflow_revisions(workflow_id: Uuid) -> String {
    format!("{API_WORKFLOWS}/{workflow_id}/revisions")
}

pub fn api_workflow_revision(workflow_id: Uuid, revision: i64) -> String {
    format!("{API_WORKFLOWS}/{workflow_id}/revisions/{revision}")
}

pub fn api_workflow_revision_restore(workflow_id: Uuid, revision: i64) -> String {
    format!("{API_WORKFLOWS}/{workflow_id}/revisions/{revision}/restore")
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

pub fn api_workflow_trigger_backfill(trigger_id: Uuid) -> String {
    format!("/workflow_triggers/{trigger_id}/backfill")
}

pub fn api_freeze_window(window_id: Uuid) -> String {
    format!("{API_FREEZE_WINDOWS}/{window_id}")
}

pub fn api_calendar_subscription(subscription_id: Uuid) -> String {
    format!("{API_CALENDAR_SUBSCRIPTIONS}/{subscription_id}")
}

pub fn api_subscribed_calendar(token: &str) -> String {
    format!("/calendar/{token}/runinator.ics")
}

pub fn api_pipeline(pipeline_id: Uuid) -> String {
    format!("{API_PIPELINES}/{pipeline_id}")
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

pub fn api_workflow_run_replay_plan(workflow_run_id: Uuid) -> String {
    format!("{API_WORKFLOW_RUNS}/{workflow_run_id}/replay-plan")
}

pub fn api_workflow_contract_impact(workflow_id: Uuid) -> String {
    format!("{API_WORKFLOWS}/{workflow_id}/contract-impact")
}

pub fn api_workflow_run_command(workflow_run_id: Uuid, command: &str) -> String {
    format!("{API_WORKFLOW_RUNS}/{workflow_run_id}/{command}")
}

pub fn api_workflow_run_continuations(workflow_run_id: Uuid) -> String {
    format!("{API_WORKFLOW_RUNS}/{workflow_run_id}/continuations")
}

pub fn api_workflow_run_effects(workflow_run_id: Uuid) -> String {
    format!("{API_WORKFLOW_RUNS}/{workflow_run_id}/effects")
}

pub fn api_workflow_run_journal(workflow_run_id: Uuid) -> String {
    format!("{API_WORKFLOW_RUNS}/{workflow_run_id}/journal")
}

/// Author-facing graph positions, projected from continuation instruction pointers and the
/// immutable module source map.
pub fn api_workflow_run_cursors(workflow_run_id: Uuid) -> String {
    format!("{API_WORKFLOW_RUNS}/{workflow_run_id}/cursors")
}

pub fn api_workflow_continuation(continuation_id: Uuid) -> String {
    format!("{API_WORKFLOW_CONTINUATIONS}/{continuation_id}")
}

pub fn api_workflow_effect(effect_id: Uuid) -> String {
    format!("{API_WORKFLOW_EFFECTS}/{effect_id}")
}

pub fn api_workflow_effect_output(effect_id: Uuid) -> String {
    format!("{API_WORKFLOW_EFFECTS}/{effect_id}/output")
}

pub fn api_workflow_run_transitions(workflow_run_id: Uuid) -> String {
    format!("{API_WORKFLOW_RUNS}/{workflow_run_id}/transitions")
}

pub fn api_scheduler_workflow_run_claim_renew(workflow_run_id: Uuid) -> String {
    format!("/scheduler/workflow_runs/{workflow_run_id}/claim/renew")
}

pub fn api_scheduler_workflow_run_claim_release(workflow_run_id: Uuid) -> String {
    format!("/scheduler/workflow_runs/{workflow_run_id}/claim/release")
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
