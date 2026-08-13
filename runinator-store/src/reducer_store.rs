//! the persistence surface the workflow state machine needs.
//!
//! `runinator-reducer` calls 28 of `DatabaseImpl`'s 200-plus operations. naming that subset lets the
//! reducer depend on what it uses instead of the whole store, which is what makes an in-memory fake
//! small enough to write and keeps sqlx out of the state machine's compile graph.
//!
//! `DatabaseImpl` has this as a supertrait, so every existing `T: DatabaseImpl` caller already
//! satisfies it and nothing needs re-plumbing.

use chrono::{DateTime, Utc};
use runinator_comm::{ActionCommand, ActionDispatchRecord};
use runinator_models::value::Value;
use runinator_models::workflow_state::WorkflowExecutionState;
use runinator_models::{
    billing::OrgResourceGroup,
    cursor::RunCursor,
    errors::SendableError,
    orchestration::{NewOrchestrationEvent, ReadyNodeRecord},
    orgs::Organization,
    pipelines::{Pipeline, PipelineRun, PipelineTrigger},
    replicas::{ReplicaKind, ReplicaRecord, ReplicaStatus, WorkflowRunProvenance},
    settings::{SettingKind, SettingRecord},
    workflows::{
        NewWorkflowRunArtifact, WorkflowDefinition, WorkflowNodeRun, WorkflowNodeRunArtifact,
        WorkflowRun, WorkflowRunArtifact, WorkflowStatus, WorkflowTrigger,
    },
};
use std::future::Future;
use uuid::Uuid;

use crate::workflow_mutex::{WorkflowMutexClaim, WorkflowMutexClaimResult, WorkflowMutexWake};

/// the store operations the reducer's node handlers call.
pub trait ReducerStore: Send + Sync + 'static {
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

    /// Update the top-level status of a pipeline run.
    fn update_pipeline_run_status(
        &self,
        pipeline_run_id: Uuid,
        status: WorkflowStatus,
        state: Option<Value>,
        message: Option<String>,
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

    /// Normalize every legacy `workflow_runs.state` value and clear it after a successful write.
    fn migrate_workflow_execution_states(
        &self,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

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

    /// Join a named mutex's FIFO queue and atomically acquire it when this node run is first.
    fn claim_workflow_mutex(
        &self,
        claim: WorkflowMutexClaim,
        now_unix: i64,
    ) -> impl Future<Output = Result<WorkflowMutexClaimResult, SendableError>> + Send;

    /// Release a named mutex only when the supplied cursor owns it, returning the oldest waiter.
    fn release_workflow_mutex(
        &self,
        name: String,
        workflow_run_id: Uuid,
        cursor_id: Uuid,
        now_unix: i64,
    ) -> impl Future<Output = Result<Option<WorkflowMutexWake>, SendableError>> + Send;

    /// Release every mutex held by a terminal/canceled run and return each oldest waiter.
    fn release_workflow_mutexes(
        &self,
        workflow_run_id: Uuid,
        now_unix: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowMutexWake>, SendableError>> + Send;

    /// Remove a node run that timed out or otherwise left a mutex queue without acquiring.
    fn remove_workflow_mutex_waiter(
        &self,
        workflow_node_run_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Create a new node execution record within a workflow run. `prev_node_run_id` is the
    /// origin node run this one transitioned from (the reducer supplies it; `None` for the
    /// first node or when unknown).
    ///
    /// `cursor` is the thread of control producing it, and supplies both the attribution and the
    /// speculative marker. passing the cursor rather than the two columns it implies is what keeps
    /// a node run from ever claiming a cursor it did not come from. `None` for callers outside the
    /// state machine (the api's compatibility endpoints), which have no cursor.
    fn create_workflow_node_run(
        &self,
        workflow_run_id: Uuid,
        node_id: String,
        parameters: Value,
        prev_node_run_id: Option<Uuid>,
        cursor: Option<&RunCursor>,
    ) -> impl Future<Output = Result<WorkflowNodeRun, SendableError>> + Send;

    /// Update the status and state of a specific node execution.
    #[allow(clippy::too_many_arguments)]
    fn update_workflow_node_run(
        &self,
        node_run_id: Uuid,
        status: WorkflowStatus,
        attempt: Option<i64>,
        parameters: Option<Value>,
        output_json: Option<Value>,
        state: Option<Value>,
        transition_reason: Option<String>,
        message: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch all node execution records for a workflow run.
    fn fetch_workflow_node_runs(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowNodeRun>, SendableError>> + Send;

    /// Clear the current executor and record the last executor for a node run. A no-op unless
    /// `replica_id` is the current holder, so a stray release cannot free another replica's lease.
    fn release_workflow_node_run_executor(
        &self,
        node_run_id: Uuid,
        replica_id: Uuid,
        released_at: DateTime<Utc>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Promote a node artifact to a run-level artifact via an output node.
    fn add_workflow_run_artifact(
        &self,
        artifact: &NewWorkflowRunArtifact,
    ) -> impl Future<Output = Result<WorkflowRunArtifact, SendableError>> + Send;

    /// Enqueue a state-machine node for scheduler processing.
    fn enqueue_ready_node(
        &self,
        event: NewOrchestrationEvent,
        node_id: String,
        ready_at: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<ReadyNodeRecord>, SendableError>> + Send;

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

    /// Store an action dispatch intent for durable scheduler recovery.
    fn enqueue_action_dispatch(
        &self,
        dedupe_key: String,
        command: ActionCommand,
    ) -> impl Future<Output = Result<ActionDispatchRecord, SendableError>> + Send;

    /// List every stored setting (encrypted values included), ordered by kind/scope/name.
    fn list_settings(
        &self,
    ) -> impl Future<Output = Result<Vec<SettingRecord>, SendableError>> + Send;

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
    ) -> impl Future<Output = Result<PipelineRun, SendableError>> + Send;

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

    /// Fetch all node execution records in a given status across every run. Used to route an
    /// inbound signal to a parked node by correlation key without knowing its run id.
    fn fetch_workflow_node_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> impl Future<Output = Result<Vec<WorkflowNodeRun>, SendableError>> + Send;

    /// Fetch every node artifact produced across a whole workflow run.
    fn fetch_workflow_node_run_artifacts_for_run(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowNodeRunArtifact>, SendableError>> + Send;

    /// Fetch replicas filtered by type and status, deriving stale state from heartbeat age.
    fn fetch_replicas(
        &self,
        replica_type: Option<ReplicaKind>,
        status: Option<ReplicaStatus>,
        stale_before: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<ReplicaRecord>, SendableError>> + Send;

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
