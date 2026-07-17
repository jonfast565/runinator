use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::fmt;
use std::ops::Deref;
use uuid::Uuid;

use crate::value::{Map, Value};

use crate::replicas::{TriggerActorType, TriggerSourceKind};
use crate::semver::{SemVer, SemVerBump};
use crate::types::RuninatorType;
use crate::workflow_ast::ConditionNode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub id: Option<Uuid>,
    pub name: String,
    /// the namespace that qualifies this workflow's identity, from a `namespace <path>` header.
    /// `None` for an unqualified workflow. a subflow target `"<namespace>.<name>"` resolves against
    /// the qualified identity `namespace + "." + name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// the organization (tenant) that owns this workflow. `None` means platform-global / unassigned,
    /// which keeps pre-tenancy workflows working unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub version: SemVer,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    #[serde(alias = "input_schema", deserialize_with = "deserialize_workflow_type")]
    pub input_type: RuninatorType,
    #[serde(default)]
    pub definition: WorkflowGraph,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct WorkflowGraph {
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub nodes: Vec<WorkflowNode>,
    #[serde(default, rename = "$defs")]
    pub defs: Map,
    #[serde(default)]
    pub metadata: Value,
    #[serde(flatten)]
    pub extra: Map,
}

impl WorkflowGraph {
    pub fn as_value(&self) -> Value {
        serde_json::to_value(self)
            .map(Value::from)
            .unwrap_or_else(|_| Value::Object(Map::new()))
    }

    pub fn from_value(value: Value) -> Result<Self, String> {
        match serde_json::from_value(value.clone().into()) {
            Ok(graph) => Ok(graph),
            Err(_) => {
                let mut expanded = value;
                expand_local_defs_refs(&mut expanded, &mut Vec::new())?;
                serde_json::from_value(expanded.into()).map_err(|err| err.to_string())
            }
        }
    }
}

fn expand_local_defs_refs(value: &mut Value, stack: &mut Vec<String>) -> Result<(), String> {
    let defs = value
        .get("$defs")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    expand_refs_in_value(value, &defs, stack)
}

fn expand_refs_in_value(
    value: &mut Value,
    defs: &Value,
    stack: &mut Vec<String>,
) -> Result<(), String> {
    match value {
        Value::Object(map) => {
            if let Some(reference) = map.get("$ref").and_then(Value::as_str).map(str::to_string) {
                if let Some(pointer) = reference.strip_prefix("#/$defs/") {
                    if stack.iter().any(|item| item == &reference) {
                        return Err(format!("detected local $ref cycle for '{reference}'"));
                    }
                    let path = format!("/{pointer}");
                    let mut replacement = defs
                        .pointer(&path)
                        .cloned()
                        .ok_or_else(|| format!("missing local $ref '{reference}'"))?;
                    stack.push(reference.clone());
                    expand_refs_in_value(&mut replacement, defs, stack)?;
                    stack.pop();
                    for (key, overlay) in map.clone() {
                        if key != "$ref"
                            && key != "with"
                            && let Value::Object(replacement_map) = &mut replacement
                        {
                            replacement_map.insert(key, overlay);
                        }
                    }
                    if let Some(with) = map.get("with") {
                        merge_overlay(&mut replacement, with.clone());
                    }
                    *value = replacement;
                    return Ok(());
                }
            }
            for nested in map.values_mut() {
                expand_refs_in_value(nested, defs, stack)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                expand_refs_in_value(item, defs, stack)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn merge_overlay(target: &mut Value, overlay: Value) {
    match (target, overlay) {
        (Value::Object(target), Value::Object(overlay)) => {
            for (key, value) in overlay {
                match target.get_mut(&key) {
                    Some(existing) => merge_overlay(existing, value),
                    None => {
                        target.insert(key, value);
                    }
                }
            }
        }
        (target, overlay) => *target = overlay,
    }
}

impl fmt::Display for WorkflowGraph {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_value().fmt(formatter)
    }
}

fn deserialize_workflow_type<'de, D>(deserializer: D) -> Result<RuninatorType, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    serde_json::from_value(value.clone().into())
        .or_else(|_| Ok(RuninatorType::from_json_schema(&value)))
}

/// request body for duplicating a workflow into a new version sharing the same name.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowDuplicateRequest {
    #[serde(default)]
    pub bump: SemVerBump,
}

/// request body for a server-side dry-run (branch preview). The `workflow` is walked with the
/// reducer's evaluators against live config, publishing no actions; `inputs` seed the run and an
/// optional `replay_run` replays that run's recorded node outputs so the walk follows real branches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowSimulateRequest {
    pub workflow: WorkflowDefinition,
    #[serde(default)]
    pub inputs: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_run: Option<Uuid>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowBundle {
    #[serde(default)]
    pub workflows: Vec<WorkflowDefinition>,
    #[serde(default)]
    pub triggers: Vec<WorkflowTrigger>,
}

// note: raw json workflow bundles use an explicit client method because the server requires
// a risk-acknowledgment header before accepting them.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTriggerKind {
    Cron,
    Manual,
    /// fire when a source workflow run reaches a terminal state (workflow-to-workflow chaining).
    /// the trigger belongs to the source workflow; the target lives in `configuration`.
    Chained,
}

impl WorkflowTriggerKind {
    /// every trigger kind in a stable, ui-facing order.
    pub const ALL: [WorkflowTriggerKind; 3] = [
        WorkflowTriggerKind::Cron,
        WorkflowTriggerKind::Manual,
        WorkflowTriggerKind::Chained,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            WorkflowTriggerKind::Cron => "cron",
            WorkflowTriggerKind::Manual => "manual",
            WorkflowTriggerKind::Chained => "chained",
        }
    }
}

impl TryFrom<&str> for WorkflowTriggerKind {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "cron" => Ok(WorkflowTriggerKind::Cron),
            "manual" => Ok(WorkflowTriggerKind::Manual),
            "chained" => Ok(WorkflowTriggerKind::Chained),
            other => Err(format!("Unknown workflow trigger kind '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTrigger {
    pub id: Option<Uuid>,
    pub workflow_id: Uuid,
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

#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowObject(Value);

impl WorkflowObject {
    pub fn as_value(&self) -> &Value {
        &self.0
    }

    pub fn into_value(self) -> Value {
        self.0
    }

    pub fn into_object(self) -> Map {
        self.0.as_object().cloned().unwrap_or_default()
    }

    pub fn from_value(value: Value) -> Result<Self, String> {
        match value {
            Value::Null => Ok(Self(Value::Object(Map::new()))),
            Value::Object(_) => Ok(Self(value)),
            _ => Err("value must be an object".into()),
        }
    }
}

impl Default for WorkflowObject {
    fn default() -> Self {
        Self(Value::Object(Map::new()))
    }
}

impl Deref for WorkflowObject {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        self.as_value()
    }
}

impl Serialize for WorkflowObject {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for WorkflowObject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        WorkflowObject::from_value(value).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for WorkflowObject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl From<WorkflowObject> for Value {
    fn from(value: WorkflowObject) -> Self {
        value.into_value()
    }
}

/// a node/branch condition: a typed `ConditionNode` tree, or `None` for the null "always true" case.
/// serializes through `Value` so the wire json is byte-identical to the untyped form it replaced.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WorkflowCondition(Option<ConditionNode>);

impl WorkflowCondition {
    /// the typed condition, or `None` when the condition is null (unconditional).
    pub fn node(&self) -> Option<&ConditionNode> {
        self.0.as_ref()
    }

    /// whether there is no condition (the null, always-true case).
    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    /// the wire `Value` form: null when empty, otherwise the condition object.
    pub fn to_value(&self) -> Value {
        match &self.0 {
            None => Value::Null,
            Some(node) => Value::from(node),
        }
    }

    /// build from a wire value: null yields the empty (always-true) condition; an object is parsed
    /// into the typed tree (unknown shapes are preserved verbatim by `ConditionNode`).
    pub fn from_value(value: Value) -> Self {
        match value {
            Value::Null => Self(None),
            other => Self(Some(ConditionNode::from(&other))),
        }
    }
}

impl From<WorkflowCondition> for Value {
    fn from(value: WorkflowCondition) -> Self {
        value.to_value()
    }
}

impl Serialize for WorkflowCondition {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for WorkflowCondition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::Null | Value::Object(_) => Ok(Self::from_value(value)),
            _ => Err(serde::de::Error::custom(
                "condition must be null or an object",
            )),
        }
    }
}

impl fmt::Display for WorkflowCondition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.to_value().fmt(formatter)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum WorkflowWaitSeconds {
    Integer(i64),
    Expression(WorkflowObject),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct WorkflowWait {
    #[serde(default)]
    pub seconds: Option<WorkflowWaitSeconds>,
    #[serde(default)]
    pub until_status: Option<String>,
    #[serde(default)]
    pub initial_status: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatus {
    Queued,
    Running,
    Paused,
    DebugPaused,
    Waiting,
    ApprovalRequired,
    InputRequired,
    Blocked,
    Succeeded,
    Failed,
    TimedOut,
    Canceled,
}

impl WorkflowStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            WorkflowStatus::Queued => "queued",
            WorkflowStatus::Running => "running",
            WorkflowStatus::Paused => "paused",
            WorkflowStatus::DebugPaused => "debug_paused",
            WorkflowStatus::Waiting => "waiting",
            WorkflowStatus::ApprovalRequired => "approval_required",
            WorkflowStatus::InputRequired => "input_required",
            WorkflowStatus::Blocked => "blocked",
            WorkflowStatus::Succeeded => "succeeded",
            WorkflowStatus::Failed => "failed",
            WorkflowStatus::TimedOut => "timed_out",
            WorkflowStatus::Canceled => "canceled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            WorkflowStatus::Succeeded
                | WorkflowStatus::Failed
                | WorkflowStatus::TimedOut
                | WorkflowStatus::Canceled
        )
    }

    pub fn is_active(self) -> bool {
        matches!(
            self,
            WorkflowStatus::Queued
                | WorkflowStatus::Running
                | WorkflowStatus::DebugPaused
                | WorkflowStatus::Waiting
                | WorkflowStatus::ApprovalRequired
                | WorkflowStatus::InputRequired
        )
    }
}

impl TryFrom<&str> for WorkflowStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "queued" => Ok(WorkflowStatus::Queued),
            "running" => Ok(WorkflowStatus::Running),
            "paused" => Ok(WorkflowStatus::Paused),
            "debug_paused" => Ok(WorkflowStatus::DebugPaused),
            "waiting" => Ok(WorkflowStatus::Waiting),
            "approval_required" => Ok(WorkflowStatus::ApprovalRequired),
            "input_required" => Ok(WorkflowStatus::InputRequired),
            "blocked" => Ok(WorkflowStatus::Blocked),
            "succeeded" => Ok(WorkflowStatus::Succeeded),
            "failed" => Ok(WorkflowStatus::Failed),
            "timed_out" => Ok(WorkflowStatus::TimedOut),
            "canceled" => Ok(WorkflowStatus::Canceled),
            other => Err(format!("Unknown workflow status '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeKind {
    Start,
    Action,
    Wait,
    Condition,
    Switch,
    /// route to `on` or `off` based on the truthiness of a single value (a literal light switch).
    Toggle,
    /// route to one of several weighted buckets by a stable hash of a key (percentage rollouts).
    Percentage,
    Approval,
    Gate,
    Signal,
    Loop,
    Parallel,
    Join,
    Try,
    Map,
    Race,
    #[serde(rename = "output", alias = "deliverable")]
    Output,
    Input,
    Subflow,
    Config,
    End,
    Fail,
    // --- new node kinds (easiest → most complex) ---
    /// evaluate named boolean assertions; fails with a structured violation list.
    Assert,
    /// resolve named expression bindings into the run context; no side effects.
    Transform,
    /// append a tamper-evident audit record to the workflow audit log.
    Audit,
    /// snapshot run state at a named point; enables rollback via the control-plane api.
    Checkpoint,
    /// acquire a named distributed mutex; parks until the lock is available.
    Mutex,
    /// enforce a cross-run rate limit; parks until a token is available.
    Throttle,
    /// wait for one or more independently-started workflow runs to reach a terminal state.
    AwaitRun,
    /// park for a trailing delay that resets when re-triggered; collapses event bursts.
    Debounce,
    /// accumulate externally-delivered items until a count or time threshold is met.
    Collect,
    /// park until N runs reach this named barrier; the last arrival releases all waiters.
    Barrier,
    /// track failure rates across runs; fast-fail or route to fallback when tripped.
    CircuitBreaker,
    /// subscribe to a named event stream; drives a body subgraph on each matching event.
    EventSource,
}

impl WorkflowNodeKind {
    /// every node kind in a stable, ui-facing order. used to build the metadata catalog; the
    /// catalog's per-kind `match` is what guarantees exhaustiveness at compile time.
    pub const ALL: [WorkflowNodeKind; 34] = [
        WorkflowNodeKind::Start,
        WorkflowNodeKind::Action,
        WorkflowNodeKind::Wait,
        WorkflowNodeKind::Condition,
        WorkflowNodeKind::Switch,
        WorkflowNodeKind::Toggle,
        WorkflowNodeKind::Percentage,
        WorkflowNodeKind::Approval,
        WorkflowNodeKind::Gate,
        WorkflowNodeKind::Signal,
        WorkflowNodeKind::Loop,
        WorkflowNodeKind::Parallel,
        WorkflowNodeKind::Join,
        WorkflowNodeKind::Try,
        WorkflowNodeKind::Map,
        WorkflowNodeKind::Race,
        WorkflowNodeKind::Output,
        WorkflowNodeKind::Input,
        WorkflowNodeKind::Subflow,
        WorkflowNodeKind::Config,
        WorkflowNodeKind::Assert,
        WorkflowNodeKind::Transform,
        WorkflowNodeKind::Audit,
        WorkflowNodeKind::Checkpoint,
        WorkflowNodeKind::Mutex,
        WorkflowNodeKind::Throttle,
        WorkflowNodeKind::AwaitRun,
        WorkflowNodeKind::Debounce,
        WorkflowNodeKind::Collect,
        WorkflowNodeKind::Barrier,
        WorkflowNodeKind::CircuitBreaker,
        WorkflowNodeKind::EventSource,
        WorkflowNodeKind::End,
        WorkflowNodeKind::Fail,
    ];
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorkflowAction {
    pub provider: String,
    pub function: String,
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: i64,
    #[serde(default)]
    pub configuration: WorkflowObject,
    #[serde(default)]
    pub mcp_enabled: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    /// routing labels a worker must carry to receive this action. empty means the general pool. the
    /// reducer maps a non-empty selector to a labelled broker target and parks until a matching worker
    /// is live.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub required_labels: BTreeMap<String, String>,
}

fn default_timeout_seconds() -> i64 {
    60
}

impl<'de> Deserialize<'de> for WorkflowAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawWorkflowAction {
            pub provider: String,
            pub function: String,
            #[serde(default = "default_timeout_seconds")]
            pub timeout_seconds: i64,
            #[serde(default)]
            pub configuration: Value,
            #[serde(default)]
            pub mcp_enabled: bool,
            #[serde(default)]
            pub tags: Vec<String>,
            #[serde(default)]
            pub required_labels: BTreeMap<String, String>,
            #[serde(flatten)]
            pub extra: Map,
        }

        let raw = RawWorkflowAction::deserialize(deserializer)?;
        if raw.extra.contains_key("metadata") {
            return Err(serde::de::Error::custom(
                "action metadata is no longer supported; use action configuration",
            ));
        }
        let configuration = merge_action_configuration(raw.configuration, raw.extra)
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            provider: raw.provider,
            function: raw.function,
            timeout_seconds: raw.timeout_seconds,
            configuration,
            mcp_enabled: raw.mcp_enabled,
            tags: raw.tags,
            required_labels: raw.required_labels,
        })
    }
}

fn merge_action_configuration(configuration: Value, extra: Map) -> Result<WorkflowObject, String> {
    if extra.is_empty() {
        return WorkflowObject::from_value(configuration);
    }
    let mut merged = match configuration {
        Value::Object(object) => object,
        Value::Null => Map::new(),
        _ => return Err("action configuration must be an object".into()),
    };
    for (key, value) in extra {
        merged.entry(key).or_insert(value);
    }
    Ok(WorkflowObject(Value::Object(merged)))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkflowNodeId(String);

impl WorkflowNodeId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for WorkflowNodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl From<String> for WorkflowNodeId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for WorkflowNodeId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorkflowNodeRef(WorkflowNodeId);

impl WorkflowNodeRef {
    pub fn new(value: impl Into<String>) -> Self {
        Self(WorkflowNodeId::new(value))
    }

    pub fn id(&self) -> &WorkflowNodeId {
        &self.0
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn into_string(self) -> String {
        self.0.into_string()
    }
}

impl Serialize for WorkflowNodeRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("$node", self.as_str())?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for WorkflowNodeRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("node reference must be an object"))?;
        if object.len() != 1 || !object.contains_key("$node") {
            return Err(serde::de::Error::custom(
                "node reference must be { \"$node\": \"node_id\" }",
            ));
        }
        let node = object
            .get("$node")
            .and_then(Value::as_str)
            .filter(|node| !node.is_empty())
            .ok_or_else(|| serde::de::Error::custom("$node must be a non-empty string"))?;
        Ok(Self::new(node))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowRetry {
    #[serde(default = "default_max_attempts")]
    pub max_attempts: i64,
    /// first-retry delay in seconds; doubles each subsequent attempt up to `backoff_max_seconds`.
    #[serde(default = "default_backoff_base_seconds")]
    pub backoff_base_seconds: i64,
    /// upper bound on the computed backoff delay in seconds.
    #[serde(default = "default_backoff_max_seconds")]
    pub backoff_max_seconds: i64,
    /// when true, the computed delay is randomized in `[delay/2, delay]` to spread retry storms.
    #[serde(default)]
    pub jitter: bool,
    /// which terminal statuses are eligible for retry. defaults to retrying both failures and
    /// timeouts; narrow it so, e.g., a long expensive action is not blindly re-run on timeout.
    #[serde(default)]
    pub retry_on: WorkflowRetryClass,
}

impl Default for WorkflowRetry {
    fn default() -> Self {
        Self {
            max_attempts: default_max_attempts(),
            backoff_base_seconds: default_backoff_base_seconds(),
            backoff_max_seconds: default_backoff_max_seconds(),
            jitter: false,
            retry_on: WorkflowRetryClass::default(),
        }
    }
}

/// classifies which terminal statuses a node is willing to retry.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRetryClass {
    /// retry both `Failed` and `TimedOut` (the historical behavior).
    #[default]
    Any,
    /// retry `Failed` only; let a timeout fall straight through to its transition.
    Failure,
    /// retry `TimedOut` only; let an outright failure fall straight through.
    Timeout,
}

impl WorkflowRetryClass {
    /// true when a node run ending in `status` is eligible for retry under this policy.
    pub fn retryable(&self, status: WorkflowStatus) -> bool {
        match self {
            Self::Any => matches!(status, WorkflowStatus::Failed | WorkflowStatus::TimedOut),
            Self::Failure => status == WorkflowStatus::Failed,
            Self::Timeout => status == WorkflowStatus::TimedOut,
        }
    }
}

fn default_max_attempts() -> i64 {
    1
}

fn default_backoff_base_seconds() -> i64 {
    1
}

fn default_backoff_max_seconds() -> i64 {
    300
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct WorkflowReentry {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub max_visits: i64,
    #[serde(default)]
    pub on_exhausted: Option<WorkflowNodeRef>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowSubflowType {
    #[default]
    Wait,
    FireAndForget,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct WorkflowSubflow {
    #[serde(default)]
    pub workflow_name: Option<String>,
    #[serde(default)]
    pub run_name: Option<Value>,
    #[serde(default)]
    pub reuse_open_run: bool,
    #[serde(default, rename = "type")]
    pub subflow_type: WorkflowSubflowType,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct WorkflowTransitions {
    #[serde(default)]
    pub next: Option<WorkflowNodeRef>,
    #[serde(default)]
    pub on_success: Option<WorkflowNodeRef>,
    #[serde(default)]
    pub on_failure: Option<WorkflowNodeRef>,
    #[serde(default)]
    pub on_timeout: Option<WorkflowNodeRef>,
    #[serde(default)]
    pub on_reject: Option<WorkflowNodeRef>,
    #[serde(default)]
    pub branches: Vec<WorkflowBranch>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowBranch {
    pub when: WorkflowCondition,
    pub target: WorkflowNodeRef,
    /// selection priority for predicate edges; lower numbers are evaluated first. unset branches
    /// keep their declaration order (sorted after any numbered branches).
    #[serde(default)]
    pub priority: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowNode {
    pub id: String,
    pub kind: WorkflowNodeKind,
    #[serde(default)]
    pub skipped: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub action: Option<WorkflowAction>,
    #[serde(default)]
    pub parameters: WorkflowObject,
    #[serde(default)]
    pub wait: WorkflowWait,
    #[serde(default)]
    pub condition: WorkflowCondition,
    #[serde(default)]
    pub transitions: WorkflowTransitions,
    #[serde(default)]
    pub retry: WorkflowRetry,
    #[serde(default)]
    pub timeout_seconds: Option<i64>,
    #[serde(default)]
    pub max_iterations: Option<i64>,
    #[serde(default)]
    pub subflow_id: Option<Uuid>,
    #[serde(default)]
    pub subflow: WorkflowSubflow,
    #[serde(default)]
    pub reentry: WorkflowReentry,
    /// compensating action recorded when this node succeeds; run in reverse on saga rollback when a
    /// later step drives the run to a failed terminal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compensation: Option<WorkflowAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: Uuid,
    pub workflow_id: Uuid,
    #[serde(default)]
    pub workflow_snapshot: Option<WorkflowDefinition>,
    pub status: WorkflowStatus,
    pub active_node_id: Option<String>,
    pub parameters: Value,
    pub state: Value,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub message: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_source_kind: Option<TriggerSourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_actor_type: Option<TriggerActorType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_actor_replica_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_actor_display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_request_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_request_ip: Option<String>,
    #[serde(default)]
    pub trigger_metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeRun {
    pub id: Uuid,
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub status: WorkflowStatus,
    pub attempt: i64,
    pub parameters: Value,
    pub output_json: Option<Value>,
    pub state: Value,
    pub transition_reason: Option<String>,
    /// the node run created immediately before this one in the same workflow run, forming a flat,
    /// guid-linked execution chain that is easier to debug than the nested `steps` output tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_node_run_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_executor_replica_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_executor_replica_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor_claimed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor_released_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeRunChunk {
    pub id: Uuid,
    pub workflow_node_run_id: Uuid,
    pub sequence: i64,
    pub stream: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeRunArtifact {
    pub id: Uuid,
    pub workflow_node_run_id: Uuid,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uri: String,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

/// Input for promoting a node artifact to a run-level artifact via an output node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewWorkflowRunArtifact {
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub artifact_id: Uuid,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uri: String,
    pub metadata: Value,
}

/// A run-level artifact declared by an output node, making it visible at workflow-run scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunArtifact {
    pub id: Uuid,
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub artifact_id: Uuid,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uri: String,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}
