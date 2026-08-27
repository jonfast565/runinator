//! the persistence surface the workflow state machine needs.
//!
//! `runinator-runtime` calls 28 of `DatabaseImpl`'s 200-plus operations. naming that subset lets the
//! runtime depend on what it uses instead of the whole store, which is what makes an in-memory fake
//! small enough to write and keeps sqlx out of the state machine's compile graph.
//!
//! `DatabaseImpl` has this as a supertrait, so every existing `T: DatabaseImpl` caller already
//! satisfies it and nothing needs re-plumbing.

use runinator_models::value::Value;
use runinator_models::workflow_state::WorkflowExecutionState;
use runinator_models::{
    billing::OrgResourceGroup,
    errors::SendableError,
    orgs::Organization,
    pipelines::{
        Pipeline, PipelineExecutionContext, PipelineMemberAttempt, PipelineMemberAttemptStatus,
        PipelineRun, PipelineTrigger,
    },
    replicas::WorkflowRunProvenance,
    settings::{SettingKind, SettingRecord},
    workflows::{WorkflowDefinition, WorkflowRun, WorkflowStatus, WorkflowTrigger},
};
use std::future::Future;
use uuid::Uuid;

use crate::roles::NewWorkflowVmRun;

/// the cross-domain persistence the engine's run, trigger, and pipeline orchestration reaches for.
///
/// it deliberately spans several domains: keeping one use-case trait small is what makes the
/// in-memory fake practical. an operation that belongs to exactly one domain goes on that domain's
/// role instead.
pub trait RuntimeStore: Send + Sync + 'static {
    /// Transitional pipeline seam: atomically bootstrap a compiled member workflow. Pipeline
    /// orchestration supplies the already-compiled module so persistence never depends on the
    /// graph compiler. Removed together with `RuntimeStore` at the legacy-runtime cutover.
    fn bootstrap_workflow_vm_run(
        &self,
        start: NewWorkflowVmRun,
    ) -> impl Future<Output = Result<WorkflowRun, SendableError>> + Send;

    /// Read the terminal value recorded by the VM for pipeline parameter propagation.
    fn fetch_workflow_vm_result(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Option<Value>, SendableError>> + Send;

    /// Fetch a workflow definition by its identifier.
    fn fetch_workflow(
        &self,
        workflow_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkflowDefinition>, SendableError>> + Send;

    /// Fetch all triggers for a workflow definition.
    fn fetch_workflow_triggers(
        &self,
        workflow_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowTrigger>, SendableError>> + Send;

    /// Fetch a pipeline instance by identifier.
    fn fetch_pipeline(
        &self,
        pipeline_id: Uuid,
    ) -> impl Future<Output = Result<Option<Pipeline>, SendableError>> + Send;

    /// Fetch every enabled `chained` pipeline trigger, for the terminal-run chaining scan.
    fn fetch_enabled_chained_pipeline_triggers(
        &self,
    ) -> impl Future<Output = Result<Vec<PipelineTrigger>, SendableError>> + Send;

    /// Fetch a pipeline run by identifier.
    fn fetch_pipeline_run(
        &self,
        pipeline_run_id: Uuid,
    ) -> impl Future<Output = Result<Option<PipelineRun>, SendableError>> + Send;

    fn fetch_pipeline_runs_for_concurrency(
        &self,
        pipeline_id: Uuid,
    ) -> impl Future<Output = Result<Vec<PipelineRun>, SendableError>> + Send;

    /// Update the top-level status of a pipeline run.
    fn update_pipeline_run_status(
        &self,
        pipeline_run_id: Uuid,
        status: WorkflowStatus,
        state: Option<Value>,
        message: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Reopen a terminal pipeline run for a frontier retry.
    fn reopen_pipeline_run(
        &self,
        pipeline_run_id: Uuid,
        message: String,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Tag a workflow run as a member of a pipeline run.
    fn set_workflow_run_pipeline_run(
        &self,
        workflow_run_id: Uuid,
        pipeline_run_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch all runs for a specific workflow definition.
    fn fetch_workflow_runs_for_workflow(
        &self,
        workflow_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;

    /// Update the top-level status of a workflow run.
    fn update_workflow_run_status(
        &self,
        workflow_run_id: Uuid,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Option<WorkflowExecutionState>,
        message: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Replace a run's state blob only if its version still matches `expected_version`, bumping the
    /// version on success. Returns false when another writer moved the row first, meaning the caller
    /// must re-read and reapply its change.
    fn update_workflow_run_execution_state_cas(
        &self,
        workflow_run_id: Uuid,
        expected_version: i64,
        state: WorkflowExecutionState,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Apply a run's status, position and state in one guarded write, only if the state version
    /// still matches. Returns false when another writer moved the row first.
    ///
    /// One statement, so the cursor list and the position mirrored into `active_node_id` can never
    /// be observed disagreeing — a fan-out is either wholly applied or not at all.
    #[allow(clippy::too_many_arguments)]
    fn update_workflow_run_status_cas(
        &self,
        workflow_run_id: Uuid,
        expected_version: i64,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: WorkflowExecutionState,
        message: Option<String>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Set or clear the user-facing display name of a workflow run.
    fn set_workflow_run_name(
        &self,
        workflow_run_id: Uuid,
        name: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Stamp a run's correlation key write-once; a run that already has one is left untouched.
    fn set_run_correlation_key(
        &self,
        workflow_run_id: Uuid,
        correlation_key: String,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch a workflow run summary by its identifier.
    fn fetch_workflow_run(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkflowRun>, SendableError>> + Send;

    /// Atomically claim a named cooldown window, admitting at most one caller per window.
    ///
    /// Returns `None` when this caller took the window (and stamped it to `now_unix`), or
    /// `Some(remaining_seconds)` when another already holds it. The decision and the stamp are one
    /// statement: reading the window and then writing it lets two concurrent runs both observe an
    /// elapsed window and both enter the body, which defeats the gate entirely.
    fn claim_cooldown(
        &self,
        name: String,
        window_seconds: i64,
        now_unix: i64,
    ) -> impl Future<Output = Result<Option<i64>, SendableError>> + Send;

    /// Create a new record in a generic orchestration table.
    fn create_automation_record(
        &self,
        record_type: String,
        record: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Update an existing orchestration record.
    fn update_automation_record(
        &self,
        record_type: String,
        record_id: Uuid,
        record: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Create a gate row (a per-run, per-node automated/policy block).
    fn create_gate(
        &self,
        record: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Update an existing gate row (status/reason/resolution).
    fn update_gate(
        &self,
        gate_id: Uuid,
        record: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Fetch a single gate row by its identifier.
    fn fetch_gate(
        &self,
        gate_id: Uuid,
    ) -> impl Future<Output = Result<Option<Value>, SendableError>> + Send;

    /// Append an audit-log entry (auth/authz/sensitive op).
    fn record_audit_log(
        &self,
        record: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// List every stored setting (encrypted values included), ordered by kind/scope/name.
    fn list_settings(
        &self,
    ) -> impl Future<Output = Result<Vec<SettingRecord>, SendableError>> + Send;

    /// Fetch a setting through its durable logical UUID. The default keeps lightweight fakes
    /// source-compatible; SQL backends may override it with an indexed lookup later.
    fn fetch_setting_by_id(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<SettingRecord>, SendableError>> + Send {
        async move {
            Ok(self
                .list_settings()
                .await?
                .into_iter()
                .find(|setting| setting.id == id))
        }
    }

    /// Fetch an org by id.
    fn fetch_org(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<Organization>, SendableError>> + Send;

    /// An org's dedicated allocations.
    fn list_org_resource_groups(
        &self,
        org_id: Uuid,
    ) -> impl Future<Output = Result<Vec<OrgResourceGroup>, SendableError>> + Send;

    /// Fetch a workflow definition by its unique display name.
    fn fetch_workflow_by_name(
        &self,
        name: String,
    ) -> impl Future<Output = Result<Option<WorkflowDefinition>, SendableError>> + Send;

    /// Record a one-off pipeline trigger firing keyed on `(trigger_id, fire_key)`, returning `true`
    /// only when this call inserted the row (chained-to-pipeline exactly-once, per source run).
    fn try_record_pipeline_trigger_firing(
        &self,
        trigger_id: Uuid,
        fire_key: String,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Create a new pipeline run (status `queued`).
    fn create_pipeline_run(
        &self,
        pipeline_id: Uuid,
        pipeline_snapshot: Pipeline,
        parameters: Value,
        state: Value,
        provenance: WorkflowRunProvenance,
        execution: PipelineExecutionContext,
    ) -> impl Future<Output = Result<PipelineRun, SendableError>> + Send;

    /// Delete a still-queued pipeline run rejected by a concurrency `skip` decision.
    fn discard_queued_pipeline_run(
        &self,
        pipeline_run_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Atomically claim a member attempt. `None` means another driver already created it.
    fn create_pipeline_member_attempt(
        &self,
        pipeline_run_id: Uuid,
        member_key: String,
        workflow_id: Uuid,
        attempt: i64,
        parameters: Value,
    ) -> impl Future<Output = Result<Option<PipelineMemberAttempt>, SendableError>> + Send;

    fn bind_pipeline_member_attempt_run(
        &self,
        attempt_id: Uuid,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    fn update_pipeline_member_attempt(
        &self,
        attempt_id: Uuid,
        status: PipelineMemberAttemptStatus,
        result: Value,
        message: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    fn fetch_pipeline_member_attempts(
        &self,
        pipeline_run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<PipelineMemberAttempt>, SendableError>> + Send;

    /// Remove derived skipped/resolution-failed attempts that never started a workflow run.
    fn delete_unstarted_pipeline_member_attempts(
        &self,
        pipeline_run_id: Uuid,
        member_key: String,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch every member workflow run tagged with the given pipeline run.
    fn fetch_workflow_runs_for_pipeline_run(
        &self,
        pipeline_run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;

    /// Record a one-off trigger firing keyed on `(trigger_id, fire_key)`, returning `true` only when
    /// this call inserted the row. used by workflow-to-workflow chaining to start a target at most
    /// once per source run (the caller creates the run only when this returns `true`).
    fn try_record_trigger_firing(
        &self,
        trigger_id: Uuid,
        fire_key: String,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Create a new instance of a workflow.
    fn create_workflow_run(
        &self,
        workflow_id: Uuid,
        workflow_snapshot: WorkflowDefinition,
        parameters: Value,
        state: Value,
        name: Option<String>,
        provenance: WorkflowRunProvenance,
    ) -> impl Future<Output = Result<WorkflowRun, SendableError>> + Send;

    /// Fetch workflow runs by display name, optionally restricted to open runs.
    fn fetch_workflow_runs_by_name(
        &self,
        name: String,
        open_only: bool,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;

    /// Fetch orchestration records with optional filters.
    fn fetch_automation_records(
        &self,
        record_type: String,
        workflow_run_id: Option<Uuid>,
        external_item_id: Option<Uuid>,
    ) -> impl Future<Output = Result<Vec<Value>, SendableError>> + Send;

    /// Fetch a single setting's persisted record, or None when it does not exist.
    fn fetch_setting(
        &self,
        kind: SettingKind,
        scope: String,
        name: String,
    ) -> impl Future<Output = Result<Option<SettingRecord>, SendableError>> + Send;
}
