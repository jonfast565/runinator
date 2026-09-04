use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::replicas::{TriggerActorType, TriggerSourceKind};
use crate::schedules::WorkflowConcurrency;
use crate::value::Value;
use crate::workflows::{WorkflowRun, WorkflowStatus, WorkflowTriggerKind};

/// what happens to downstream links when a member workflow fails. authoring-only: it seeds the
/// `on` selector of newly drawn links (`Halt` -> fire on success, `Continue` -> fire on complete).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineFailurePolicy {
    /// stop the pipeline when a step fails (new links default to firing on success).
    #[default]
    Halt,
    /// keep going when a step fails (new links default to firing on complete).
    Continue,
}

impl PipelineFailurePolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            PipelineFailurePolicy::Halt => "halt",
            PipelineFailurePolicy::Continue => "continue",
        }
    }
}

/// what happens to the *pipeline run* when one of its member workflows fails, evaluated per graph
/// member (falling back to [`PipelineDefaults::default_failure_mode`] during import). Named after PowerShell's `$ErrorActionPreference`,
/// whose `Stop`/`Continue`/`SilentlyContinue`/`Inquire` values this mirrors one-for-one. Unlike
/// [`PipelineFailurePolicy`] (which only seeds a newly-drawn link's `on` selector), this is enforced
/// at runtime by the chaining/settlement orchestration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineMemberFailureMode {
    /// the failed member fires none of its outgoing links; the pipeline run still settles once
    /// every already-started member quiesces, and this failure counts toward that settlement.
    Stop,
    /// the failed member's outgoing links still fire per their own `on` selector (today's
    /// behavior), and this failure counts toward the pipeline run's settlement. the default, so an
    /// existing pipeline's behavior is unchanged by this setting's introduction.
    #[default]
    Continue,
    /// like `Continue`, but this member's failure alone does not fail the pipeline run's
    /// settlement (another member's `Stop`/`Continue` failure still can).
    SilentlyContinue,
    /// the failed member fires none of its outgoing links until a human resolves the pipeline
    /// run's pending inquiry (continue or abort); the pipeline run pauses (`approval_required`)
    /// rather than settling while the inquiry is open.
    Inquire,
}

impl PipelineMemberFailureMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PipelineMemberFailureMode::Stop => "stop",
            PipelineMemberFailureMode::Continue => "continue",
            PipelineMemberFailureMode::SilentlyContinue => "silently_continue",
            PipelineMemberFailureMode::Inquire => "inquire",
        }
    }
}

/// editable pipeline-level defaults applied when authoring links inside a pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineDefaults {
    #[serde(default)]
    pub on_step_failure: PipelineFailurePolicy,
    #[serde(default = "default_true")]
    pub links_enabled_by_default: bool,
    #[serde(default)]
    pub default_parameters: Value,
    #[serde(default)]
    pub max_chain_depth: Option<u32>,
    /// the failure mode copied onto a member that omits one during import.
    #[serde(default)]
    pub default_failure_mode: PipelineMemberFailureMode,
}

fn default_true() -> bool {
    true
}

impl Default for PipelineDefaults {
    fn default() -> Self {
        PipelineDefaults {
            on_step_failure: PipelineFailurePolicy::default(),
            links_enabled_by_default: true,
            default_parameters: Value::default(),
            max_chain_depth: None,
            default_failure_mode: PipelineMemberFailureMode::default(),
        }
    }
}

/// which terminal state of a source member fires the link to the next member. mirrors the `on`
/// selector of a `chained` workflow trigger (`success` / `complete` / `failure`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineLinkSelector {
    /// fire only when the source run succeeds.
    #[default]
    Success,
    /// fire when the source run reaches any terminal state.
    Complete,
    /// fire only when the source run fails or times out.
    Failure,
}

impl PipelineLinkSelector {
    /// the chained-trigger `on` string this selector maps to.
    pub fn as_str(&self) -> &'static str {
        match self {
            PipelineLinkSelector::Success => "success",
            PipelineLinkSelector::Complete => "complete",
            PipelineLinkSelector::Failure => "failure",
        }
    }
}

/// a directed link between two member workflows (by canonical path), realized as a `chained` trigger on the
/// `from` workflow targeting the `to` workflow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineLinkSpec {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub on: PipelineLinkSelector,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// pure expression object over `params`, `source`, and `members`, overlaid onto pipeline input.
    #[serde(default)]
    pub parameters: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineJoinMode {
    #[default]
    All,
    Any,
    FirstSuccess,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineJoinSpec {
    pub target: String,
    pub mode: PipelineJoinMode,
    #[serde(default)]
    pub parameters: Value,
}

/// a portable, id-free pipeline declaration compiled from a `.rexrapp` file. members and links use
/// canonical workflow paths; the web service resolves those paths to ids and persists one atomic graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineSpec {
    pub name: String,
    /// Stable key used to find this logical pipeline across display-name edits and namespace moves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub defaults: PipelineDefaults,
    #[serde(default)]
    pub members: Vec<PipelineMemberSpec>,
    #[serde(default)]
    pub links: Vec<PipelineLinkSpec>,
    #[serde(default)]
    pub joins: Vec<PipelineJoinSpec>,
    #[serde(default)]
    pub concurrency: WorkflowConcurrency,
    /// Portable pipeline metadata authored with the pack.  Importers add their managed markers
    /// without replacing this object, so generic policies can travel with the declaration.
    #[serde(default)]
    pub metadata: Value,
    /// pipeline-level triggers (cron / manual / chained) declared in the `.rexrapp` header. materialized
    /// on import as managed `pipeline_triggers` reconciled by pipeline id.
    #[serde(default)]
    pub triggers: Vec<PipelineTriggerSpec>,
}

/// a member workflow declared in a `.rexrapp` pipeline, by canonical `namespace.key` path.
/// `failure_mode` is `None` when the
/// member declares no `on_failure` of its own, meaning it takes the pipeline's
/// [`PipelineDefaults::default_failure_mode`] at import.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineMemberSpec {
    pub name: String,
    #[serde(default)]
    pub failure_mode: Option<PipelineMemberFailureMode>,
}

/// a bare name is a member with no failure-mode override (takes the pipeline default at import).
impl From<&str> for PipelineMemberSpec {
    fn from(name: &str) -> Self {
        PipelineMemberSpec {
            name: name.to_string(),
            failure_mode: None,
        }
    }
}

impl From<String> for PipelineMemberSpec {
    fn from(name: String) -> Self {
        PipelineMemberSpec {
            name,
            failure_mode: None,
        }
    }
}

/// a portable, id-free pipeline trigger declaration compiled from a `.rexrapp` header. `configuration`
/// carries kind-specific data (cron: `{cron, parameters}`; chained: `{on, source_workflow |
/// source_pipeline, source_workflow_id | source_pipeline_id, parameters}`); manual triggers carry
/// no schedule. The path is authored for diagnostics; the resolved UUID is authoritative at runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineTriggerSpec {
    pub kind: WorkflowTriggerKind,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub configuration: Value,
}

// pipelinedefaults derives clone but not partialeq; pipelinespec's partialeq needs it.
impl PartialEq for PipelineDefaults {
    fn eq(&self, other: &Self) -> bool {
        self.on_step_failure == other.on_step_failure
            && self.links_enabled_by_default == other.links_enabled_by_default
            && self.default_parameters == other.default_parameters
            && self.max_chain_depth == other.max_chain_depth
            && self.default_failure_mode == other.default_failure_mode
    }
}

/// the compiled pipeline artifact carried in a pack zip as `pipelines.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PipelineBundle {
    #[serde(default)]
    pub pipelines: Vec<PipelineSpec>,
}

pub const PIPELINE_GRAPH_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineMember {
    /// stable pipeline-local identity and expression key: the authored canonical workflow path.
    pub key: String,
    pub workflow_id: Uuid,
    #[serde(default)]
    pub failure_mode: PipelineMemberFailureMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineLink {
    pub id: Uuid,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub on: PipelineLinkSelector,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub parameters: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineJoin {
    pub target: String,
    pub mode: PipelineJoinMode,
    #[serde(default)]
    pub parameters: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PipelineGraph {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub members: Vec<PipelineMember>,
    #[serde(default)]
    pub links: Vec<PipelineLink>,
    #[serde(default)]
    pub joins: BTreeMap<String, PipelineJoin>,
}

impl PipelineGraph {
    pub fn is_current(&self) -> bool {
        self.version == PIPELINE_GRAPH_VERSION
    }
}

/// a named pipeline instance: a chosen set of member workflows plus authoring defaults. the links
/// between members remain `chained` workflow triggers stamped with this pipeline's id; the runtime
/// chaining engine is unaware of pipelines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub id: Option<Uuid>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// the owning organization (tenant), or `None` for platform-global. stamped from the creator's
    /// active org on create and preserved on update.
    #[serde(default)]
    pub org_id: Option<Uuid>,
    /// Whether this pipeline may admit new manual, trigger, or ingress runs.
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub graph: PipelineGraph,
    #[serde(default)]
    pub concurrency: WorkflowConcurrency,
    #[serde(default)]
    pub defaults: PipelineDefaults,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

impl Pipeline {
    pub fn artifact_key(&self) -> &str {
        self.key.as_deref().unwrap_or(&self.name)
    }

    pub fn artifact_path(&self) -> crate::artifacts::ArtifactPath {
        crate::artifacts::ArtifactPath::new(self.namespace.clone(), self.artifact_key().to_string())
    }
}

impl crate::validation::Validate for Pipeline {
    fn validate(&self) -> Result<(), crate::validation::ValidationError> {
        use crate::validation::{
            LONG_TEXT_MAX, SHORT_TEXT_MAX, ValidationError, identifier, optional_text,
            required_text, serialized,
        };

        required_text("name", &self.name, SHORT_TEXT_MAX)?;
        if let Some(key) = self.key.as_deref() {
            identifier("key", key)?;
        }
        optional_text("namespace", self.namespace.as_deref(), SHORT_TEXT_MAX)?;
        optional_text("description", self.description.as_deref(), LONG_TEXT_MAX)?;
        if self.concurrency.max_concurrent_runs < 0 {
            return Err(ValidationError::new(
                "concurrency.max_concurrent_runs",
                "must not be negative",
            ));
        }
        for (index, member) in self.graph.members.iter().enumerate() {
            required_text(
                &format!("graph.members[{index}].key"),
                &member.key,
                SHORT_TEXT_MAX,
            )?;
        }
        for (index, link) in self.graph.links.iter().enumerate() {
            required_text(
                &format!("graph.links[{index}].from"),
                &link.from,
                SHORT_TEXT_MAX,
            )?;
            required_text(
                &format!("graph.links[{index}].to"),
                &link.to,
                SHORT_TEXT_MAX,
            )?;
        }
        serialized("pipeline", self)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineMemberAttemptStatus {
    Pending,
    Queued,
    Running,
    Waiting,
    ApprovalRequired,
    Succeeded,
    Failed,
    TimedOut,
    Canceled,
    Skipped,
}

impl PipelineMemberAttemptStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::ApprovalRequired => "approval_required",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Canceled => "canceled",
            Self::Skipped => "skipped",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::TimedOut | Self::Canceled | Self::Skipped
        )
    }
}

impl TryFrom<&str> for PipelineMemberAttemptStatus {
    type Error = String;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "pending" => Ok(Self::Pending),
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "waiting" => Ok(Self::Waiting),
            "approval_required" => Ok(Self::ApprovalRequired),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "timed_out" => Ok(Self::TimedOut),
            "canceled" => Ok(Self::Canceled),
            "skipped" => Ok(Self::Skipped),
            other => Err(format!("unknown pipeline member attempt status {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineMemberAttempt {
    pub id: Uuid,
    pub pipeline_run_id: Uuid,
    pub member_key: String,
    pub workflow_id: Uuid,
    pub attempt: i64,
    pub workflow_run_id: Option<Uuid>,
    pub status: PipelineMemberAttemptStatus,
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub result: Value,
    pub message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRunEdgeState {
    pub link_id: Uuid,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRunJoinState {
    pub target: String,
    pub mode: PipelineJoinMode,
    pub state: String,
    pub satisfied_inputs: usize,
    pub total_inputs: usize,
}

/// a persisted pipeline-level trigger. mirrors [`crate::workflows::WorkflowTrigger`] but is owned by a
/// pipeline: cron/manual start a pipeline run for `pipeline_id`; a `chained` trigger is target-keyed
/// (`pipeline_id` is the pipeline to start) with its source and `on` selector in `configuration`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineTrigger {
    pub id: Option<Uuid>,
    pub pipeline_id: Uuid,
    pub kind: WorkflowTriggerKind,
    pub enabled: bool,
    #[serde(default)]
    pub configuration: Value,
    pub next_execution: Option<DateTime<Utc>>,
    pub blackout_start: Option<DateTime<Utc>>,
    pub blackout_end: Option<DateTime<Utc>>,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

impl crate::validation::Validate for PipelineTrigger {
    fn validate(&self) -> Result<(), crate::validation::ValidationError> {
        crate::workflows::validate_trigger_window(self.blackout_start, self.blackout_end)?;
        crate::validation::dynamic_value("configuration", &self.configuration)?;
        crate::validation::dynamic_value("metadata", &self.metadata)?;
        Ok(())
    }
}

/// a first-class pipeline execution. an orchestration envelope over the member workflow runs it
/// starts: each member run is stamped with this run's id, and the run settles when the reachable
/// member graph reaches terminal. status reuses [`WorkflowStatus`] (queued, running, parked,
/// sleeping, and the terminal states are meaningful for a pipeline run).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRun {
    pub id: Uuid,
    pub pipeline_id: Uuid,
    #[serde(default)]
    pub pipeline_snapshot: Option<Pipeline>,
    pub status: WorkflowStatus,
    pub parameters: Value,
    pub state: Value,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_source_kind: Option<TriggerSourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_actor_type: Option<TriggerActorType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_actor_replica_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_actor_display_name: Option<String>,
    #[serde(default)]
    pub trigger_metadata: Value,
    /// Present when this immutable run is one epoch of a correlated orchestration binding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orchestration_binding_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_epoch: Option<i64>,
    /// Optional member chosen as the sole initial frontier for a resumed/superseding epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_member: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PipelineExecutionContext {
    pub orchestration_binding_id: Option<Uuid>,
    pub execution_epoch: Option<i64>,
    pub start_member: Option<String>,
}

/// a pipeline run with the member workflow runs it started. mirrors the workflow-run detail shape so
/// the UI can render the same list+detail layout and click through from a member step to its run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRunDetail {
    pub run: PipelineRun,
    pub members: Vec<WorkflowRun>,
    #[serde(default)]
    pub attempts: Vec<PipelineMemberAttempt>,
    #[serde(default)]
    pub edges: Vec<PipelineRunEdgeState>,
    #[serde(default)]
    pub joins: Vec<PipelineRunJoinState>,
}
