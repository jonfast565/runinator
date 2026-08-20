//! an in-memory `RuntimeStore` for testing node operations.
//!
//! the runtime's 35 node operations were previously reachable only through the web service's test
//! suite, which boots a real sqlite database: the highest-value logic in the product was tested at
//! the highest-cost layer, and several park/release bugs shipped because no cheap test could reach
//! them. narrowing the runtime's bound to [`RuntimeStore`] made this fake possible — 40 methods
//! rather than the store's full 200-plus.
//!
//! only the operations handlers actually exercise carry behavior. the rest return empty results, and
//! the few that cannot return anything sensible panic with a message saying so, which keeps the fake
//! honest: a handler that starts depending on one fails loudly instead of silently reading a zero.
//!
//! enabled by the `test-support` feature, mirroring `runinator-engine`.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use runinator_comm::{ActionCommand, ActionDispatchRecord};
use runinator_models::billing::OrgResourceGroup;
use runinator_models::cursor::RunCursor;
use runinator_models::errors::SendableError;
use runinator_models::orchestration::{NewOrchestrationEvent, ReadyNodeRecord};
use runinator_models::orgs::Organization;
use runinator_models::pipelines::{
    Pipeline, PipelineMemberAttempt, PipelineMemberAttemptStatus, PipelineRun, PipelineTrigger,
};
use runinator_models::replicas::{
    ReplicaKind, ReplicaRecord, ReplicaStatus, WorkflowRunProvenance,
};
use runinator_models::runs::{
    NewRunArtifact, NewRunChunk, RunArtifact, RunChunk, RunStatus, RunSummary,
};
use runinator_models::settings::{SettingKind, SettingRecord};
use runinator_models::value::Value;
use runinator_models::workflow_state::WorkflowExecutionState;
use runinator_models::workflows::{
    NewWorkflowRunArtifact, WorkflowAction, WorkflowDefinition, WorkflowNodeRun,
    WorkflowNodeRunArtifact, WorkflowRun, WorkflowRunArtifact, WorkflowStatus, WorkflowTaskRun,
    WorkflowTrigger,
};
use runinator_store::RuntimeStore;
use runinator_store::roles::TaskRunStore;
use runinator_store::workflow_mutex::{
    WorkflowMutexClaim, WorkflowMutexClaimResult, WorkflowMutexWake,
};
use uuid::Uuid;

mod invocations;
use invocations::InvocationState;

#[derive(Clone)]
struct FakeMutexHolder {
    workflow_run_id: Uuid,
    cursor_id: Uuid,
    hold_deadline_unix: Option<i64>,
    overdue: bool,
}

/// everything the fake remembers between calls.
#[derive(Default)]
struct State {
    workflows: Vec<WorkflowDefinition>,
    pipelines: Vec<Pipeline>,
    triggers: Vec<WorkflowTrigger>,
    runs: HashMap<Uuid, WorkflowRun>,
    pipeline_runs: HashMap<Uuid, PipelineRun>,
    pipeline_attempts: Vec<PipelineMemberAttempt>,
    node_runs: Vec<WorkflowNodeRun>,
    workflow_task_runs: HashMap<Uuid, WorkflowTaskRun>,
    run_artifacts: Vec<WorkflowRunArtifact>,
    ready_nodes: Vec<ReadyNodeRecord>,
    dispatches: Vec<ActionDispatchRecord>,
    audit: Vec<Value>,
    /// named cooldown windows: `name -> last_run_at`.
    cooldowns: HashMap<String, i64>,
    mutex_holders: HashMap<String, FakeMutexHolder>,
    mutex_waiters: HashMap<Uuid, WorkflowMutexClaim>,
    /// keyed by record type, matching `automation_records` rows.
    automation: HashMap<String, Vec<Value>>,
    /// the `InvocationStore` half, kept in its own struct beside its impl.
    invocations_state: InvocationState,
}

/// an in-memory store for driving node operations in a unit test.
#[derive(Default)]
pub struct FakeStore {
    state: Mutex<State>,
}

impl FakeStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// register a workflow definition handlers can resolve by id.
    pub fn insert_workflow(&self, workflow: WorkflowDefinition) {
        self.state.lock().expect("state").workflows.push(workflow);
    }

    pub fn insert_pipeline(&self, pipeline: Pipeline) {
        self.state.lock().expect("state").pipelines.push(pipeline);
    }

    pub fn pipeline_attempts(&self, pipeline_run_id: Uuid) -> Vec<PipelineMemberAttempt> {
        self.state
            .lock()
            .expect("state")
            .pipeline_attempts
            .iter()
            .filter(|attempt| attempt.pipeline_run_id == pipeline_run_id)
            .cloned()
            .collect()
    }

    pub fn insert_pipeline_run(&self, run: PipelineRun) {
        self.state
            .lock()
            .expect("state")
            .pipeline_runs
            .insert(run.id, run);
    }

    pub fn insert_pipeline_attempt(&self, attempt: PipelineMemberAttempt) {
        self.state
            .lock()
            .expect("state")
            .pipeline_attempts
            .push(attempt);
    }

    /// register a trigger on a workflow, for the chaining path that reads them on a terminal.
    pub fn insert_trigger(&self, trigger: WorkflowTrigger) {
        self.state.lock().expect("state").triggers.push(trigger);
    }

    /// flip a stored workflow's `enabled` flag, the switch the schedule and the chain both read.
    pub fn set_workflow_enabled(&self, workflow_id: Uuid, enabled: bool) {
        let mut guard = self.state.lock().expect("state");
        if let Some(workflow) = guard
            .workflows
            .iter_mut()
            .find(|workflow| workflow.id == Some(workflow_id))
        {
            workflow.enabled = enabled;
        }
    }

    /// register a run. returns the run id for convenience.
    pub fn insert_run(&self, mut run: WorkflowRun) -> Uuid {
        let id = run.id;
        run.execution_state = WorkflowExecutionState::from_state(&run.state);
        self.state.lock().expect("state").runs.insert(id, run);
        id
    }

    pub fn insert_node_run(&self, node_run: WorkflowNodeRun) {
        self.state.lock().expect("state").node_runs.push(node_run);
    }

    pub fn insert_run_artifact(&self, artifact: WorkflowRunArtifact) {
        self.state
            .lock()
            .expect("state")
            .run_artifacts
            .push(artifact);
    }

    pub fn run(&self, id: Uuid) -> Option<WorkflowRun> {
        self.state.lock().expect("state").runs.get(&id).cloned()
    }

    pub fn node_runs(&self) -> Vec<WorkflowNodeRun> {
        self.state.lock().expect("state").node_runs.clone()
    }

    /// the latest recorded run for a node id, which is what most assertions look at.
    pub fn latest_node_run(&self, node_id: &str) -> Option<WorkflowNodeRun> {
        self.state
            .lock()
            .expect("state")
            .node_runs
            .iter()
            .rfind(|run| run.node_id == node_id)
            .cloned()
    }

    /// ready-node rows enqueued by handlers, in insertion order. a parked node arms its timeout by
    /// enqueueing one of these, so their presence is how a test observes parking.
    pub fn ready_nodes(&self) -> Vec<ReadyNodeRecord> {
        self.state.lock().expect("state").ready_nodes.clone()
    }

    /// action commands the runtime wrote to the dispatch outbox.
    pub fn dispatches(&self) -> Vec<ActionDispatchRecord> {
        self.state.lock().expect("state").dispatches.clone()
    }

    /// backdate a node run's `created_at` so a timeout can be exercised without sleeping.
    ///
    /// deliberately does not touch `started_at`: a parked node never has one, and that asymmetry is
    /// the whole point of the park-timeout tests.
    pub fn age_node_run(&self, node_run_id: Uuid, by: chrono::Duration) {
        let mut guard = self.state.lock().expect("state");
        if let Some(run) = guard.node_runs.iter_mut().find(|run| run.id == node_run_id) {
            run.created_at -= by;
            // a dispatched node's deadline runs from `started_at`, a park's from `created_at`.
            // moving only one of them would leave half the timeout checks looking at the present.
            if let Some(started) = run.started_at.as_mut() {
                *started -= by;
            }
        }
    }

    /// settle a run the way its own runtime drive would, for a parent waiting on a child.
    pub fn settle_run(&self, run_id: Uuid, status: WorkflowStatus) {
        let mut guard = self.state.lock().expect("state");
        if let Some(run) = guard.runs.get_mut(&run_id) {
            run.status = status;
            run.finished_at = Some(Utc::now());
        }
    }

    /// every run this store holds, oldest first, for finding a child a node spawned.
    pub fn runs(&self) -> Vec<WorkflowRun> {
        let mut runs: Vec<WorkflowRun> = self
            .state
            .lock()
            .expect("state")
            .runs
            .values()
            .cloned()
            .collect();
        runs.sort_by_key(|run| run.id);
        runs
    }

    /// stamp a parked node run the way an out-of-band delivery endpoint would.
    pub fn resolve_node_run(
        &self,
        node_run_id: Uuid,
        status: WorkflowStatus,
        output: Option<Value>,
    ) {
        let mut guard = self.state.lock().expect("state");
        if let Some(run) = guard.node_runs.iter_mut().find(|run| run.id == node_run_id) {
            run.status = status;
            run.output_json = output;
            run.finished_at = Some(Utc::now());
        }
    }

    /// replace a node run's durable handler state for deadline and recovery tests.
    pub fn set_node_run_state(&self, node_run_id: Uuid, state: Value) {
        let mut guard = self.state.lock().expect("state");
        if let Some(run) = guard.node_runs.iter_mut().find(|run| run.id == node_run_id) {
            run.state = state.into();
        }
    }

    /// the `last_run_at` a named cooldown window currently holds.
    pub fn cooldown_window(&self, name: &str) -> Option<i64> {
        self.state
            .lock()
            .expect("state")
            .cooldowns
            .get(name)
            .copied()
    }

    /// open a cooldown window directly, for a test that needs a gate already held.
    pub fn seed_cooldown(&self, name: &str, last_run_at: i64) {
        self.state
            .lock()
            .expect("state")
            .cooldowns
            .insert(name.to_string(), last_run_at);
    }

    /// insert an automation row directly, for a test that needs a gate already stamped.
    pub async fn seed_automation_record(&self, record_type: &str, record: Value) {
        use runinator_store::RuntimeStore;
        self.create_automation_record(record_type.to_string(), record)
            .await
            .expect("seed automation record");
    }

    /// automation rows of a type, newest last. the cooldown gate's window lives here.
    pub fn automation_records(&self, record_type: &str) -> Vec<Value> {
        self.state
            .lock()
            .expect("state")
            .automation
            .get(record_type)
            .cloned()
            .unwrap_or_default()
    }

    pub fn audit_records(&self) -> Vec<Value> {
        self.state.lock().expect("state").audit.clone()
    }
}

impl RuntimeStore for FakeStore {
    async fn fetch_workflow(
        &self,
        workflow_id: Uuid,
    ) -> Result<Option<WorkflowDefinition>, SendableError> {
        Ok(self
            .state
            .lock()
            .expect("state")
            .workflows
            .iter()
            .find(|workflow| workflow.id == Some(workflow_id))
            .cloned())
    }

    async fn fetch_workflow_run(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Option<WorkflowRun>, SendableError> {
        Ok(self
            .state
            .lock()
            .expect("state")
            .runs
            .get(&workflow_run_id)
            .cloned())
    }

    async fn update_workflow_run_status(
        &self,
        workflow_run_id: Uuid,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Option<WorkflowExecutionState>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let mut guard = self.state.lock().expect("state");
        if let Some(run) = guard.runs.get_mut(&workflow_run_id) {
            run.status = status;
            // a `None` active node means "leave it where it is", matching the sql behaviour.
            if active_node_id.is_some() {
                run.active_node_id = active_node_id;
            }
            if let Some(state) = state {
                run.execution_state = state;
                run.state = Value::Object(Default::default());
            }
            if message.is_some() {
                run.message = message;
            }
            if status.is_terminal() {
                run.finished_at = Some(Utc::now());
            }
        }
        Ok(())
    }

    /// compare-and-swap the whole state blob. the version is bumped on every accepted write, so a
    /// caller that read an older one loses and rebuilds — the same contract the sql backend has.
    async fn update_workflow_run_execution_state_cas(
        &self,
        workflow_run_id: Uuid,
        expected_version: i64,
        state: WorkflowExecutionState,
    ) -> Result<bool, SendableError> {
        let mut guard = self.state.lock().expect("state");
        let Some(run) = guard.runs.get_mut(&workflow_run_id) else {
            return Ok(false);
        };
        if run.state_version != expected_version {
            return Ok(false);
        }
        run.execution_state = state;
        run.state = Value::Object(Default::default());
        run.state_version += 1;
        Ok(true)
    }

    async fn update_workflow_run_status_cas(
        &self,
        workflow_run_id: Uuid,
        expected_version: i64,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: WorkflowExecutionState,
        message: Option<String>,
    ) -> Result<bool, SendableError> {
        let mut guard = self.state.lock().expect("state");
        let Some(run) = guard.runs.get_mut(&workflow_run_id) else {
            return Ok(false);
        };
        if run.state_version != expected_version {
            return Ok(false);
        }
        run.status = status;
        if active_node_id.is_some() {
            run.active_node_id = active_node_id;
        }
        run.execution_state = state;
        run.state = Value::Object(Default::default());
        run.state_version += 1;
        if message.is_some() {
            run.message = message;
        }
        if status.is_terminal() {
            run.finished_at = Some(Utc::now());
        }
        Ok(true)
    }

    async fn migrate_workflow_execution_states(&self) -> Result<(), SendableError> {
        let mut guard = self.state.lock().expect("state");
        for run in guard.runs.values_mut() {
            if run.execution_state.cursors.is_empty() && !run.state.is_null() {
                run.execution_state = WorkflowExecutionState::from_state(&run.state);
            }
            run.state = Value::Object(Default::default());
        }
        Ok(())
    }

    async fn create_workflow_node_run(
        &self,
        workflow_run_id: Uuid,
        node_id: String,
        parameters: Value,
        _prev_node_run_id: Option<Uuid>,
        cursor: Option<&RunCursor>,
    ) -> Result<WorkflowNodeRun, SendableError> {
        let node_run: WorkflowNodeRun = serde_json::from_value(serde_json::json!({
            "id": Uuid::now_v7(),
            "workflow_run_id": workflow_run_id,
            "node_id": node_id,
            "cursor_id": cursor.map(|cursor| cursor.id),
            "speculative": cursor.is_some_and(RunCursor::is_speculative),
            "status": "running",
            "attempt": 1,
            "parameters": serde_json::to_value(&parameters)?,
            "output_json": null,
            "state": null,
            "transition_reason": null,
            "created_at": Utc::now(),
            "started_at": null,
            "finished_at": null,
            "message": null,
        }))?;
        self.state
            .lock()
            .expect("state")
            .node_runs
            .push(node_run.clone());
        Ok(node_run)
    }

    #[allow(clippy::too_many_arguments)]
    async fn update_workflow_node_run(
        &self,
        node_run_id: Uuid,
        status: WorkflowStatus,
        attempt: Option<i64>,
        parameters: Option<Value>,
        output_json: Option<Value>,
        state: Option<Value>,
        transition_reason: Option<String>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let mut guard = self.state.lock().expect("state");
        let Some(node_run) = guard.node_runs.iter_mut().find(|run| run.id == node_run_id) else {
            return Ok(());
        };
        node_run.status = status;
        if let Some(attempt) = attempt {
            node_run.attempt = attempt;
        }
        if let Some(parameters) = parameters {
            node_run.parameters = parameters;
        }
        if output_json.is_some() {
            node_run.output_json = output_json;
        }
        if state.is_some() {
            node_run.state = state.into();
        }
        if transition_reason.is_some() {
            node_run.transition_reason = transition_reason;
        }
        if message.is_some() {
            node_run.message = message;
        }
        // `started_at` is only stamped on Running, which is exactly the gap that made every parked
        // node kind un-timeout-able until the runtime switched to `timed_out_since_created`.
        if status == WorkflowStatus::Running && node_run.started_at.is_none() {
            node_run.started_at = Some(Utc::now());
        }
        if status.is_terminal() {
            node_run.finished_at = Some(Utc::now());
        }
        Ok(())
    }

    async fn fetch_workflow_node_runs(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowNodeRun>, SendableError> {
        Ok(self
            .state
            .lock()
            .expect("state")
            .node_runs
            .iter()
            .filter(|run| run.workflow_run_id == workflow_run_id)
            .cloned()
            .collect())
    }

    async fn fetch_promoted_workflow_run_artifacts(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowRunArtifact>, SendableError> {
        Ok(self
            .state
            .lock()
            .expect("state")
            .run_artifacts
            .iter()
            .filter(|artifact| artifact.workflow_run_id == workflow_run_id)
            .cloned()
            .collect())
    }

    async fn fetch_workflow_node_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> Result<Vec<WorkflowNodeRun>, SendableError> {
        Ok(self
            .state
            .lock()
            .expect("state")
            .node_runs
            .iter()
            .filter(|run| run.status == status)
            .cloned()
            .collect())
    }

    async fn fetch_workflow_runs_for_workflow(
        &self,
        workflow_id: Uuid,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        Ok(self
            .state
            .lock()
            .expect("state")
            .runs
            .values()
            .filter(|run| run.workflow_id == workflow_id)
            .cloned()
            .collect())
    }

    async fn enqueue_ready_node(
        &self,
        event: NewOrchestrationEvent,
        node_id: String,
        ready_at: DateTime<Utc>,
    ) -> Result<Option<ReadyNodeRecord>, SendableError> {
        let record: ReadyNodeRecord = serde_json::from_value(serde_json::json!({
            "id": Uuid::now_v7(),
            "source_event_id": Uuid::now_v7(),
            "workflow_run_id": event.workflow_run_id,
            "node_id": node_id,
            "cursor_id": event.cursor_id,
            "status": "queued",
            "ready_at": ready_at,
            "attempts": 0,
            "created_at": Utc::now(),
            "updated_at": Utc::now(),
        }))?;
        self.state
            .lock()
            .expect("state")
            .ready_nodes
            .push(record.clone());
        Ok(Some(record))
    }

    async fn enqueue_action_dispatch(
        &self,
        dedupe_key: String,
        command: ActionCommand,
    ) -> Result<ActionDispatchRecord, SendableError> {
        let record = ActionDispatchRecord {
            id: Uuid::now_v7(),
            dedupe_key,
            command,
            attempts: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            claimed_by: None,
            claimed_until: None,
            published_at: None,
            last_error: None,
        };
        self.state
            .lock()
            .expect("state")
            .dispatches
            .push(record.clone());
        Ok(record)
    }

    async fn record_audit_log(&self, record: Value) -> Result<Value, SendableError> {
        self.state.lock().expect("state").audit.push(record.clone());
        Ok(record)
    }

    async fn set_workflow_run_name(
        &self,
        workflow_run_id: Uuid,
        name: Option<String>,
    ) -> Result<(), SendableError> {
        if let Some(run) = self
            .state
            .lock()
            .expect("state")
            .runs
            .get_mut(&workflow_run_id)
        {
            run.name = name;
        }
        Ok(())
    }

    async fn set_run_correlation_key(
        &self,
        workflow_run_id: Uuid,
        correlation_key: String,
    ) -> Result<(), SendableError> {
        if let Some(run) = self
            .state
            .lock()
            .expect("state")
            .runs
            .get_mut(&workflow_run_id)
        {
            // write-once, matching the durable behaviour the await-by-correlation join relies on.
            if run.correlation_key.is_none() {
                run.correlation_key = Some(correlation_key);
            }
        }
        Ok(())
    }

    async fn release_workflow_node_run_executor(
        &self,
        _node_run_id: Uuid,
        _replica_id: Uuid,
        _released_at: DateTime<Utc>,
    ) -> Result<(), SendableError> {
        Ok(())
    }

    async fn fetch_workflow_triggers(
        &self,
        workflow_id: Uuid,
    ) -> Result<Vec<WorkflowTrigger>, SendableError> {
        Ok(self
            .state
            .lock()
            .expect("state")
            .triggers
            .iter()
            .filter(|trigger| trigger.workflow_id == workflow_id)
            .cloned()
            .collect())
    }

    async fn fetch_pipeline(&self, pipeline_id: Uuid) -> Result<Option<Pipeline>, SendableError> {
        Ok(self
            .state
            .lock()
            .expect("state")
            .pipelines
            .iter()
            .find(|pipeline| pipeline.id == Some(pipeline_id))
            .cloned())
    }

    async fn fetch_enabled_chained_pipeline_triggers(
        &self,
    ) -> Result<Vec<PipelineTrigger>, SendableError> {
        Ok(Vec::new())
    }

    async fn fetch_pipeline_run(
        &self,
        pipeline_run_id: Uuid,
    ) -> Result<Option<PipelineRun>, SendableError> {
        Ok(self
            .state
            .lock()
            .expect("state")
            .pipeline_runs
            .get(&pipeline_run_id)
            .cloned())
    }

    async fn fetch_pipeline_runs_for_concurrency(
        &self,
        pipeline_id: Uuid,
    ) -> Result<Vec<PipelineRun>, SendableError> {
        Ok(self
            .state
            .lock()
            .expect("state")
            .pipeline_runs
            .values()
            .filter(|run| run.pipeline_id == pipeline_id)
            .cloned()
            .collect())
    }

    async fn update_pipeline_run_status(
        &self,
        pipeline_run_id: Uuid,
        status: WorkflowStatus,
        state: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        if let Some(run) = self
            .state
            .lock()
            .expect("state")
            .pipeline_runs
            .get_mut(&pipeline_run_id)
        {
            run.status = status;
            if let Some(state) = state {
                run.state = state;
            }
            if let Some(message) = message {
                run.message = Some(message);
            }
            if status == WorkflowStatus::Running && run.started_at.is_none() {
                run.started_at = Some(Utc::now());
            }
            if status.is_terminal() {
                run.finished_at = Some(Utc::now());
            }
        }
        Ok(())
    }

    async fn reopen_pipeline_run(
        &self,
        pipeline_run_id: Uuid,
        message: String,
    ) -> Result<(), SendableError> {
        if let Some(run) = self
            .state
            .lock()
            .expect("state")
            .pipeline_runs
            .get_mut(&pipeline_run_id)
            && matches!(
                run.status,
                WorkflowStatus::Failed | WorkflowStatus::TimedOut
            )
        {
            run.status = WorkflowStatus::Running;
            run.finished_at = None;
            run.message = Some(message);
        }
        Ok(())
    }

    async fn set_workflow_run_pipeline_run(
        &self,
        workflow_run_id: Uuid,
        pipeline_run_id: Uuid,
    ) -> Result<(), SendableError> {
        if let Some(run) = self
            .state
            .lock()
            .expect("state")
            .runs
            .get_mut(&workflow_run_id)
        {
            run.pipeline_run_id = Some(pipeline_run_id);
        }
        Ok(())
    }

    async fn add_workflow_run_artifact(
        &self,
        _artifact: &NewWorkflowRunArtifact,
    ) -> Result<WorkflowRunArtifact, SendableError> {
        unimplemented!("FakeStore::add_workflow_run_artifact is not needed by any handler test yet")
    }

    /// stamp the row's identity into the payload exactly as `mappers::row_to_automation_record`
    /// does. a fake that returned the bare `data` would hide every bug that depends on reading the
    /// record's `id` back — which is precisely how the cooldown gate re-stamps its window.
    /// atomic by construction: the decide-and-stamp happens under the one state lock, with no
    /// await inside it, which is the fake's stand-in for the sql statement doing both at once. a
    /// fake that read and then wrote would let a concurrency test pass against a racy backend.
    async fn claim_cooldown(
        &self,
        name: String,
        window_seconds: i64,
        now_unix: i64,
    ) -> Result<Option<i64>, SendableError> {
        let cutoff = now_unix.saturating_sub(window_seconds.max(0));
        let mut guard = self.state.lock().expect("state");

        match guard.cooldowns.get(&name).copied() {
            Some(last_run_at) if last_run_at > cutoff => {
                Ok(Some((last_run_at + window_seconds - now_unix).max(0)))
            }
            _ => {
                guard.cooldowns.insert(name, now_unix);
                Ok(None)
            }
        }
    }

    async fn claim_workflow_mutex(
        &self,
        claim: WorkflowMutexClaim,
        now_unix: i64,
    ) -> Result<WorkflowMutexClaimResult, SendableError> {
        let mut guard = self.state.lock().expect("state");
        guard
            .mutex_waiters
            .insert(claim.workflow_node_run_id, claim.clone());
        let terminal = guard
            .mutex_holders
            .get(&claim.name)
            .and_then(|holder| guard.runs.get(&holder.workflow_run_id))
            .is_none_or(|run| run.status.is_terminal());
        if terminal {
            guard.mutex_holders.remove(&claim.name);
        }
        if guard.mutex_holders.contains_key(&claim.name) {
            let (same_owner, holder_overdue) = {
                let holder = guard.mutex_holders.get_mut(&claim.name).expect("holder");
                let same_owner = holder.workflow_run_id == claim.workflow_run_id
                    && holder.cursor_id == claim.cursor_id;
                if holder
                    .hold_deadline_unix
                    .is_some_and(|deadline| deadline < now_unix)
                {
                    holder.overdue = true;
                }
                (same_owner, holder.overdue)
            };
            if same_owner {
                guard.mutex_waiters.remove(&claim.workflow_node_run_id);
                return Ok(WorkflowMutexClaimResult {
                    acquired: true,
                    holder_overdue,
                    wake: None,
                });
            }
            return Ok(WorkflowMutexClaimResult {
                acquired: false,
                holder_overdue,
                wake: None,
            });
        }

        let active_runs = guard
            .runs
            .iter()
            .filter(|(_, run)| !run.status.is_terminal())
            .map(|(id, _)| *id)
            .collect::<std::collections::HashSet<_>>();
        guard
            .mutex_waiters
            .retain(|_, waiter| active_runs.contains(&waiter.workflow_run_id));
        let oldest = guard
            .mutex_waiters
            .values()
            .filter(|waiter| waiter.name == claim.name)
            .min_by_key(|waiter| (waiter.enqueued_at_unix, waiter.workflow_node_run_id))
            .cloned();
        let Some(oldest) = oldest else {
            return Ok(WorkflowMutexClaimResult {
                acquired: false,
                holder_overdue: false,
                wake: None,
            });
        };
        if oldest.workflow_node_run_id != claim.workflow_node_run_id {
            return Ok(WorkflowMutexClaimResult {
                acquired: false,
                holder_overdue: false,
                wake: Some(mutex_wake(&oldest)),
            });
        }
        guard.mutex_holders.insert(
            claim.name.clone(),
            FakeMutexHolder {
                workflow_run_id: claim.workflow_run_id,
                cursor_id: claim.cursor_id,
                hold_deadline_unix: claim.hold_deadline_unix,
                overdue: claim
                    .hold_deadline_unix
                    .is_some_and(|deadline| deadline < now_unix),
            },
        );
        guard.mutex_waiters.remove(&claim.workflow_node_run_id);
        Ok(WorkflowMutexClaimResult {
            acquired: true,
            holder_overdue: claim
                .hold_deadline_unix
                .is_some_and(|deadline| deadline < now_unix),
            wake: None,
        })
    }

    async fn release_workflow_mutex(
        &self,
        name: String,
        workflow_run_id: Uuid,
        cursor_id: Uuid,
        _now_unix: i64,
    ) -> Result<Option<WorkflowMutexWake>, SendableError> {
        let mut guard = self.state.lock().expect("state");
        let owns = guard.mutex_holders.get(&name).is_some_and(|holder| {
            holder.workflow_run_id == workflow_run_id && holder.cursor_id == cursor_id
        });
        if !owns {
            return Ok(None);
        }
        guard.mutex_holders.remove(&name);
        Ok(oldest_fake_mutex_waiter(&mut guard, &name).map(|waiter| mutex_wake(&waiter)))
    }

    async fn release_workflow_mutexes(
        &self,
        workflow_run_id: Uuid,
        _now_unix: i64,
    ) -> Result<Vec<WorkflowMutexWake>, SendableError> {
        let mut guard = self.state.lock().expect("state");
        guard
            .mutex_waiters
            .retain(|_, waiter| waiter.workflow_run_id != workflow_run_id);
        let names = guard
            .mutex_holders
            .iter()
            .filter(|(_, holder)| holder.workflow_run_id == workflow_run_id)
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        let mut wakes = Vec::new();
        for name in names {
            guard.mutex_holders.remove(&name);
            if let Some(waiter) = oldest_fake_mutex_waiter(&mut guard, &name) {
                wakes.push(mutex_wake(&waiter));
            }
        }
        Ok(wakes)
    }

    async fn remove_workflow_mutex_waiter(
        &self,
        workflow_node_run_id: Uuid,
    ) -> Result<(), SendableError> {
        self.state
            .lock()
            .expect("state")
            .mutex_waiters
            .remove(&workflow_node_run_id);
        Ok(())
    }

    /// the window a gate currently holds, for assertions.
    async fn create_automation_record(
        &self,
        record_type: String,
        record: Value,
    ) -> Result<Value, SendableError> {
        let mut stored = record;

        if let Some(object) = stored.as_object_mut() {
            object.insert("id".into(), Value::from(Uuid::now_v7().to_string()));
            object.insert("record_type".into(), Value::from(record_type.clone()));
        }

        self.state
            .lock()
            .expect("state")
            .automation
            .entry(record_type)
            .or_default()
            .push(stored.clone());
        Ok(stored)
    }

    async fn update_automation_record(
        &self,
        record_type: String,
        record_id: Uuid,
        record: Value,
    ) -> Result<Value, SendableError> {
        let mut guard = self.state.lock().expect("state");
        let rows = guard.automation.entry(record_type).or_default();
        let target = record_id.to_string();

        if let Some(slot) = rows
            .iter_mut()
            .find(|row| row.get("id").and_then(Value::as_str) == Some(target.as_str()))
        {
            *slot = record.clone();
        }

        Ok(record)
    }

    async fn create_gate(&self, _record: Value) -> Result<Value, SendableError> {
        Ok(Value::Null)
    }

    async fn update_gate(&self, _gate_id: Uuid, _record: Value) -> Result<Value, SendableError> {
        Ok(Value::Null)
    }

    async fn fetch_gate(&self, _gate_id: Uuid) -> Result<Option<Value>, SendableError> {
        Ok(None)
    }

    async fn list_settings(&self) -> Result<Vec<SettingRecord>, SendableError> {
        Ok(Vec::new())
    }

    async fn fetch_org(&self, _id: Uuid) -> Result<Option<Organization>, SendableError> {
        Ok(None)
    }

    async fn list_org_resource_groups(
        &self,
        _org_id: Uuid,
    ) -> Result<Vec<OrgResourceGroup>, SendableError> {
        Ok(Vec::new())
    }

    async fn fetch_workflow_by_name(
        &self,
        name: String,
    ) -> Result<Option<WorkflowDefinition>, SendableError> {
        Ok(self
            .state
            .lock()
            .expect("state")
            .workflows
            .iter()
            .find(|workflow| workflow.name == name)
            .cloned())
    }

    async fn try_record_pipeline_trigger_firing(
        &self,
        _trigger_id: Uuid,
        _fire_key: String,
    ) -> Result<bool, SendableError> {
        Ok(true)
    }

    async fn create_pipeline_run(
        &self,
        pipeline_id: Uuid,
        pipeline_snapshot: Pipeline,
        parameters: Value,
        state: Value,
        provenance: WorkflowRunProvenance,
    ) -> Result<PipelineRun, SendableError> {
        let run = PipelineRun {
            id: Uuid::now_v7(),
            pipeline_id,
            pipeline_snapshot: Some(pipeline_snapshot),
            status: WorkflowStatus::Queued,
            parameters,
            state,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
            message: None,
            trigger_source_kind: provenance.source_kind,
            trigger_actor_type: provenance.actor_type,
            trigger_actor_replica_id: provenance.actor_replica_id,
            trigger_actor_display_name: provenance.actor_display_name,
            trigger_metadata: provenance.metadata,
        };
        self.state
            .lock()
            .expect("state")
            .pipeline_runs
            .insert(run.id, run.clone());
        Ok(run)
    }

    async fn discard_queued_pipeline_run(
        &self,
        pipeline_run_id: Uuid,
    ) -> Result<(), SendableError> {
        let mut state = self.state.lock().expect("state");
        if state
            .pipeline_runs
            .get(&pipeline_run_id)
            .is_some_and(|run| run.status == WorkflowStatus::Queued)
        {
            state.pipeline_runs.remove(&pipeline_run_id);
        }
        Ok(())
    }

    async fn create_pipeline_member_attempt(
        &self,
        pipeline_run_id: Uuid,
        member_key: String,
        workflow_id: Uuid,
        attempt: i64,
        parameters: Value,
    ) -> Result<Option<PipelineMemberAttempt>, SendableError> {
        let mut state = self.state.lock().expect("state");
        if state.pipeline_attempts.iter().any(|row| {
            row.pipeline_run_id == pipeline_run_id
                && row.member_key == member_key
                && row.attempt == attempt
        }) {
            return Ok(None);
        }
        let row = PipelineMemberAttempt {
            id: Uuid::now_v7(),
            pipeline_run_id,
            member_key,
            workflow_id,
            attempt,
            workflow_run_id: None,
            status: PipelineMemberAttemptStatus::Pending,
            parameters,
            result: Value::Null,
            message: None,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
        };
        state.pipeline_attempts.push(row.clone());
        Ok(Some(row))
    }

    async fn bind_pipeline_member_attempt_run(
        &self,
        attempt_id: Uuid,
        workflow_run_id: Uuid,
    ) -> Result<(), SendableError> {
        if let Some(row) = self
            .state
            .lock()
            .expect("state")
            .pipeline_attempts
            .iter_mut()
            .find(|row| row.id == attempt_id)
        {
            row.workflow_run_id = Some(workflow_run_id);
            row.status = PipelineMemberAttemptStatus::Running;
            row.started_at = Some(Utc::now());
        }
        Ok(())
    }

    async fn update_pipeline_member_attempt(
        &self,
        attempt_id: Uuid,
        status: PipelineMemberAttemptStatus,
        result: Value,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        if let Some(row) = self
            .state
            .lock()
            .expect("state")
            .pipeline_attempts
            .iter_mut()
            .find(|row| row.id == attempt_id)
        {
            row.status = status;
            row.result = result;
            row.message = message;
            if status.is_terminal() {
                row.finished_at = Some(Utc::now());
            }
        }
        Ok(())
    }

    async fn fetch_pipeline_member_attempts(
        &self,
        pipeline_run_id: Uuid,
    ) -> Result<Vec<PipelineMemberAttempt>, SendableError> {
        Ok(self
            .state
            .lock()
            .expect("state")
            .pipeline_attempts
            .iter()
            .filter(|row| row.pipeline_run_id == pipeline_run_id)
            .cloned()
            .collect())
    }

    async fn delete_unstarted_pipeline_member_attempts(
        &self,
        pipeline_run_id: Uuid,
        member_key: String,
    ) -> Result<(), SendableError> {
        self.state
            .lock()
            .expect("state")
            .pipeline_attempts
            .retain(|attempt| {
                attempt.pipeline_run_id != pipeline_run_id
                    || attempt.member_key != member_key
                    || attempt.workflow_run_id.is_some()
            });
        Ok(())
    }

    async fn fetch_workflow_runs_for_pipeline_run(
        &self,
        pipeline_run_id: Uuid,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        Ok(self
            .state
            .lock()
            .expect("state")
            .runs
            .values()
            .filter(|run| run.pipeline_run_id == Some(pipeline_run_id))
            .cloned()
            .collect())
    }

    async fn try_record_trigger_firing(
        &self,
        _trigger_id: Uuid,
        _fire_key: String,
    ) -> Result<bool, SendableError> {
        Ok(true)
    }

    async fn create_workflow_run(
        &self,
        workflow_id: Uuid,
        workflow_snapshot: WorkflowDefinition,
        parameters: Value,
        state: Value,
        name: Option<String>,
        _provenance: WorkflowRunProvenance,
    ) -> Result<WorkflowRun, SendableError> {
        // enough of a run for a subflow parent to find its child and read its status. provenance is
        // dropped: nothing the runtime does reads it back.
        let mut run: WorkflowRun = serde_json::from_value(serde_json::json!({
            "id": Uuid::now_v7(),
            "workflow_id": workflow_id,
            "workflow_snapshot": workflow_snapshot,
            "status": "queued",
            "active_node_id": null,
            "parameters": parameters,
            "name": name,
            "created_at": Utc::now(),
            "started_at": null,
            "finished_at": null,
            "message": null,
        }))?;
        run.execution_state = WorkflowExecutionState::from_state(&state);
        self.state
            .lock()
            .expect("state")
            .runs
            .insert(run.id, run.clone());
        Ok(run)
    }

    async fn fetch_workflow_runs_by_name(
        &self,
        _name: String,
        _open_only: bool,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        Ok(Vec::new())
    }

    async fn fetch_workflow_node_run_artifacts_for_run(
        &self,
        _workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowNodeRunArtifact>, SendableError> {
        Ok(Vec::new())
    }

    async fn fetch_replicas(
        &self,
        _replica_type: Option<ReplicaKind>,
        _status: Option<ReplicaStatus>,
        _stale_before: DateTime<Utc>,
    ) -> Result<Vec<ReplicaRecord>, SendableError> {
        Ok(Vec::new())
    }

    async fn fetch_automation_records(
        &self,
        record_type: String,
        workflow_run_id: Option<Uuid>,
        _external_item_id: Option<Uuid>,
    ) -> Result<Vec<Value>, SendableError> {
        let guard = self.state.lock().expect("state");
        let mut rows = guard
            .automation
            .get(&record_type)
            .cloned()
            .unwrap_or_default();
        // the sql query is `ORDER BY created_at DESC, id DESC`; callers use `.find()` and so read
        // the newest matching record. a fake returning insertion order would disagree with the
        // database precisely when duplicates exist, which is when it matters.
        rows.reverse();
        Ok(rows
            .into_iter()
            .filter(|row| {
                workflow_run_id.is_none_or(|id| {
                    row.get("workflow_run_id").and_then(Value::as_str)
                        == Some(id.to_string().as_str())
                })
            })
            .collect())
    }

    async fn fetch_setting(
        &self,
        _kind: SettingKind,
        _scope: String,
        _name: String,
    ) -> Result<Option<SettingRecord>, SendableError> {
        Ok(None)
    }
}

impl TaskRunStore for FakeStore {
    async fn create_workflow_task_run(
        &self,
        workflow_run_id: Uuid,
        launch_node_run_id: Uuid,
        node_id: String,
        action: WorkflowAction,
        parameters: Value,
    ) -> Result<WorkflowTaskRun, SendableError> {
        let now = Utc::now();
        let task = WorkflowTaskRun {
            id: Uuid::now_v7(),
            workflow_run_id,
            launch_node_run_id,
            node_id,
            action,
            status: WorkflowStatus::Queued,
            attempt: 0,
            parameters,
            output_json: None,
            created_at: now,
            started_at: None,
            finished_at: None,
            message: None,
            current_executor_replica_id: None,
            last_executor_replica_id: None,
            executor_claimed_at: None,
            executor_released_at: None,
        };
        self.state
            .lock()
            .expect("state")
            .workflow_task_runs
            .insert(task.id, task.clone());
        Ok(task)
    }

    async fn fetch_workflow_task_run(
        &self,
        task_run_id: Uuid,
    ) -> Result<Option<WorkflowTaskRun>, SendableError> {
        Ok(self
            .state
            .lock()
            .expect("state")
            .workflow_task_runs
            .get(&task_run_id)
            .cloned())
    }

    async fn fetch_workflow_task_runs(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowTaskRun>, SendableError> {
        Ok(self
            .state
            .lock()
            .expect("state")
            .workflow_task_runs
            .values()
            .filter(|task| task.workflow_run_id == workflow_run_id)
            .cloned()
            .collect())
    }

    async fn update_workflow_task_run(
        &self,
        task_run_id: Uuid,
        status: WorkflowStatus,
        attempt: Option<i64>,
        output_json: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let mut state = self.state.lock().expect("state");
        let Some(task) = state.workflow_task_runs.get_mut(&task_run_id) else {
            return Err(format!("unknown fake workflow task run {task_run_id}").into());
        };
        task.status = status;
        if let Some(attempt) = attempt {
            task.attempt = attempt;
        }
        if output_json.is_some() {
            task.output_json = output_json;
        }
        if message.is_some() {
            task.message = message;
        }
        let now = Utc::now();
        if status == WorkflowStatus::Running && task.started_at.is_none() {
            task.started_at = Some(now);
        }
        if status.is_terminal() {
            task.finished_at = Some(now);
        }
        Ok(())
    }

    async fn fetch_runs_by_status(
        &self,
        _status: RunStatus,
    ) -> Result<Vec<RunSummary>, SendableError> {
        Ok(Vec::new())
    }

    async fn update_run_status(
        &self,
        _run_id: Uuid,
        _status: RunStatus,
        _output_json: Option<Value>,
        _message: Option<String>,
    ) -> Result<(), SendableError> {
        Ok(())
    }

    async fn append_run_chunk(
        &self,
        _run_id: Uuid,
        _chunk: &NewRunChunk,
    ) -> Result<RunChunk, SendableError> {
        Err("standalone task runs are not modelled by FakeStore".into())
    }

    async fn fetch_run_chunks(
        &self,
        _run_id: Uuid,
        _cursor: Option<i64>,
        _limit: i64,
    ) -> Result<Vec<RunChunk>, SendableError> {
        Ok(Vec::new())
    }

    async fn add_run_artifact(
        &self,
        _run_id: Uuid,
        _artifact: &NewRunArtifact,
    ) -> Result<RunArtifact, SendableError> {
        Err("standalone task runs are not modelled by FakeStore".into())
    }

    async fn fetch_run_artifacts(&self, _run_id: Uuid) -> Result<Vec<RunArtifact>, SendableError> {
        Ok(Vec::new())
    }

    async fn fetch_all_artifacts(&self) -> Result<Vec<RunArtifact>, SendableError> {
        Ok(Vec::new())
    }

    async fn fetch_artifact(
        &self,
        _artifact_id: Uuid,
    ) -> Result<Option<RunArtifact>, SendableError> {
        Ok(None)
    }

    async fn delete_artifact(&self, _artifact_id: Uuid) -> Result<bool, SendableError> {
        Ok(false)
    }
}

fn mutex_wake(waiter: &WorkflowMutexClaim) -> WorkflowMutexWake {
    WorkflowMutexWake {
        workflow_run_id: waiter.workflow_run_id,
        workflow_node_run_id: waiter.workflow_node_run_id,
        cursor_id: waiter.cursor_id,
        node_id: waiter.node_id.clone(),
    }
}

fn oldest_fake_mutex_waiter(state: &mut State, name: &str) -> Option<WorkflowMutexClaim> {
    let active_runs = state
        .runs
        .iter()
        .filter(|(_, run)| !run.status.is_terminal())
        .map(|(id, _)| *id)
        .collect::<std::collections::HashSet<_>>();
    state
        .mutex_waiters
        .retain(|_, waiter| active_runs.contains(&waiter.workflow_run_id));
    state
        .mutex_waiters
        .values()
        .filter(|waiter| waiter.name == name)
        .min_by_key(|waiter| (waiter.enqueued_at_unix, waiter.workflow_node_run_id))
        .cloned()
}
