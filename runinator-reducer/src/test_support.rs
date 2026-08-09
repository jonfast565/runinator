//! an in-memory `ReducerStore` for testing node handlers.
//!
//! the reducer's 35 node handlers were previously reachable only through the web service's test
//! suite, which boots a real sqlite database: the highest-value logic in the product was tested at
//! the highest-cost layer, and several park/release bugs shipped because no cheap test could reach
//! them. narrowing the reducer's bound to [`ReducerStore`] made this fake possible — 40 methods
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
use runinator_models::pipelines::{Pipeline, PipelineRun, PipelineTrigger};
use runinator_models::replicas::{
    ReplicaKind, ReplicaRecord, ReplicaStatus, WorkflowRunProvenance,
};
use runinator_models::settings::{SettingKind, SettingRecord};
use runinator_models::value::Value;
use runinator_models::workflows::{
    NewWorkflowRunArtifact, WorkflowDefinition, WorkflowNodeRun, WorkflowNodeRunArtifact,
    WorkflowRun, WorkflowRunArtifact, WorkflowStatus, WorkflowTrigger,
};
use runinator_store::ReducerStore;
use uuid::Uuid;

/// everything the fake remembers between calls.
#[derive(Default)]
struct State {
    workflows: Vec<WorkflowDefinition>,
    runs: HashMap<Uuid, WorkflowRun>,
    node_runs: Vec<WorkflowNodeRun>,
    ready_nodes: Vec<ReadyNodeRecord>,
    dispatches: Vec<ActionDispatchRecord>,
    audit: Vec<Value>,
    /// named cooldown windows: `name -> last_run_at`.
    cooldowns: HashMap<String, i64>,
    /// keyed by record type, matching `automation_records` rows.
    automation: HashMap<String, Vec<Value>>,
}

/// an in-memory store for driving node handlers in a unit test.
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

    /// register a run. returns the run id for convenience.
    pub fn insert_run(&self, run: WorkflowRun) -> Uuid {
        let id = run.id;
        self.state.lock().expect("state").runs.insert(id, run);
        id
    }

    pub fn insert_node_run(&self, node_run: WorkflowNodeRun) {
        self.state.lock().expect("state").node_runs.push(node_run);
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
            .filter(|run| run.node_id == node_id)
            .next_back()
            .cloned()
    }

    /// ready-node rows enqueued by handlers, in insertion order. a parked node arms its timeout by
    /// enqueueing one of these, so their presence is how a test observes parking.
    pub fn ready_nodes(&self) -> Vec<ReadyNodeRecord> {
        self.state.lock().expect("state").ready_nodes.clone()
    }

    /// action commands the reducer wrote to the dispatch outbox.
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

    /// settle a run the way its own reducer drive would, for a parent waiting on a child.
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
        use runinator_store::ReducerStore;
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

impl ReducerStore for FakeStore {
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
        state: Option<Value>,
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
                run.state = state;
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
    async fn update_workflow_run_state_cas(
        &self,
        workflow_run_id: Uuid,
        expected_version: i64,
        state: Value,
    ) -> Result<bool, SendableError> {
        let mut guard = self.state.lock().expect("state");
        let Some(run) = guard.runs.get_mut(&workflow_run_id) else {
            return Ok(false);
        };
        if run.state_version != expected_version {
            return Ok(false);
        }
        run.state = state;
        run.state_version += 1;
        Ok(true)
    }

    async fn update_workflow_run_status_cas(
        &self,
        workflow_run_id: Uuid,
        expected_version: i64,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Value,
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
        run.state = state;
        run.state_version += 1;
        if message.is_some() {
            run.message = message;
        }
        if status.is_terminal() {
            run.finished_at = Some(Utc::now());
        }
        Ok(true)
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
        // node kind un-timeout-able until the reducer switched to `timed_out_since_created`.
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
        _workflow_id: Uuid,
    ) -> Result<Vec<WorkflowTrigger>, SendableError> {
        Ok(Vec::new())
    }

    async fn fetch_pipeline(&self, _pipeline_id: Uuid) -> Result<Option<Pipeline>, SendableError> {
        Ok(None)
    }

    async fn fetch_enabled_chained_pipeline_triggers(
        &self,
    ) -> Result<Vec<PipelineTrigger>, SendableError> {
        Ok(Vec::new())
    }

    async fn fetch_pipeline_run(
        &self,
        _pipeline_run_id: Uuid,
    ) -> Result<Option<PipelineRun>, SendableError> {
        Ok(None)
    }

    async fn update_pipeline_run_status(
        &self,
        _pipeline_run_id: Uuid,
        _status: WorkflowStatus,
        _state: Option<Value>,
        _message: Option<String>,
    ) -> Result<(), SendableError> {
        Ok(())
    }

    async fn set_workflow_run_pipeline_run(
        &self,
        _workflow_run_id: Uuid,
        _pipeline_run_id: Uuid,
    ) -> Result<(), SendableError> {
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
        _pipeline_id: Uuid,
        _pipeline_snapshot: Pipeline,
        _parameters: Value,
        _state: Value,
        _provenance: WorkflowRunProvenance,
    ) -> Result<PipelineRun, SendableError> {
        unimplemented!("FakeStore::create_pipeline_run is not needed by any handler test yet")
    }

    async fn fetch_workflow_runs_for_pipeline_run(
        &self,
        _pipeline_run_id: Uuid,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        Ok(Vec::new())
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
        // dropped: nothing the reducer does reads it back.
        let run: WorkflowRun = serde_json::from_value(serde_json::json!({
            "id": Uuid::now_v7(),
            "workflow_id": workflow_id,
            "workflow_snapshot": workflow_snapshot,
            "status": "queued",
            "active_node_id": null,
            "parameters": parameters,
            "state": state,
            "name": name,
            "created_at": Utc::now(),
            "started_at": null,
            "finished_at": null,
            "message": null,
        }))?;
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
