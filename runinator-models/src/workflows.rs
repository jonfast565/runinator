use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::ops::Deref;

use crate::value::{Map, Value};

use crate::types::RuninatorType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub id: Option<i64>,
    pub name: String,
    pub version: i64,
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

fn expand_refs_in_value(value: &mut Value, defs: &Value, stack: &mut Vec<String>) -> Result<(), String> {
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowBundle {
    #[serde(default)]
    pub workflows: Vec<WorkflowDefinition>,
    #[serde(default)]
    pub triggers: Vec<WorkflowTrigger>,
}

impl crate::bundles::Bundle for WorkflowBundle {
    const RESOURCE: &'static str = "/workflows/import";
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTriggerKind {
    Cron,
    Manual,
}

impl WorkflowTriggerKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkflowTriggerKind::Cron => "cron",
            WorkflowTriggerKind::Manual => "manual",
        }
    }
}

impl TryFrom<&str> for WorkflowTriggerKind {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "cron" => Ok(WorkflowTriggerKind::Cron),
            "manual" => Ok(WorkflowTriggerKind::Manual),
            other => Err(format!("Unknown workflow trigger kind '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTrigger {
    pub id: Option<i64>,
    pub workflow_id: i64,
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

#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowCondition(Value);

impl WorkflowCondition {
    pub fn as_value(&self) -> &Value {
        &self.0
    }

    pub fn into_value(self) -> Value {
        self.0
    }
}

impl From<WorkflowCondition> for Value {
    fn from(value: WorkflowCondition) -> Self {
        value.into_value()
    }
}

impl Default for WorkflowCondition {
    fn default() -> Self {
        Self(Value::Null)
    }
}

impl Deref for WorkflowCondition {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        self.as_value()
    }
}

impl Serialize for WorkflowCondition {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for WorkflowCondition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::Null | Value::Object(_) => Ok(Self(value)),
            _ => Err(serde::de::Error::custom("condition must be null or an object")),
        }
    }
}

impl fmt::Display for WorkflowCondition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
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
    Approval,
    Loop,
    Parallel,
    Join,
    Try,
    Map,
    Race,
    Emit,
    Subflow,
    Config,
    End,
    Fail,
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
}

impl Default for WorkflowRetry {
    fn default() -> Self {
        Self {
            max_attempts: default_max_attempts(),
        }
    }
}

fn default_max_attempts() -> i64 {
    1
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
    pub when: Value,
    pub target: WorkflowNodeRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowNode {
    pub id: String,
    pub kind: WorkflowNodeKind,
    #[serde(default)]
    pub skipped: bool,
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
    pub subflow_id: Option<i64>,
    #[serde(default)]
    pub subflow: WorkflowSubflow,
    #[serde(default)]
    pub reentry: WorkflowReentry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: i64,
    pub workflow_id: i64,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeRun {
    pub id: i64,
    pub workflow_run_id: i64,
    pub node_id: String,
    pub status: WorkflowStatus,
    pub attempt: i64,
    pub parameters: Value,
    pub output_json: Option<Value>,
    pub state: Value,
    pub transition_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeRunChunk {
    pub id: i64,
    pub workflow_node_run_id: i64,
    pub sequence: i64,
    pub stream: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeRunArtifact {
    pub id: i64,
    pub workflow_node_run_id: i64,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uri: String,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}
