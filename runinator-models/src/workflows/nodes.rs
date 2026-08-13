use super::*;

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

impl WorkflowNode {
    /// the visit bound used by iterative control flow, regardless of its legacy wire location.
    pub fn iteration_limit(&self) -> Option<i64> {
        self.max_iterations.or_else(|| {
            (self.reentry.enabled && self.reentry.max_visits > 0).then_some(self.reentry.max_visits)
        })
    }
}
