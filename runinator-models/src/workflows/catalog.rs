use super::*;

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

    /// the statuses a run can no longer leave. exposed so sql callers can build an `IN (...)` list
    /// without restating the set and drifting from [`WorkflowStatus::is_terminal`].
    pub const TERMINAL: [WorkflowStatus; 4] = [
        WorkflowStatus::Succeeded,
        WorkflowStatus::Failed,
        WorkflowStatus::TimedOut,
        WorkflowStatus::Canceled,
    ];

    pub fn is_terminal(self) -> bool {
        Self::TERMINAL.contains(&self)
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
    /// named cross-run cooldown: a pass within the window short-circuits the run to success
    /// without running the body, so at most one pass proceeds per window.
    Cooldown,
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
    /// terminates an interrupt handler region and hands control back to the thread the interrupt
    /// suspended, choosing how that thread proceeds. legal only inside a handler region.
    Resume,
    /// begins an interrupt handler region. workflow metadata links a source to this entry.
    ///
    /// the interrupt analogue of [`WorkflowNodeKind::Start`]: a workflow has one primary entry
    /// point and one of these per declared handler. the runtime places a cursor here when it
    /// raises the interrupt; nothing may transition into it.
    Interrupt,
}

impl WorkflowNodeKind {
    /// every node kind in a stable, ui-facing order. used to build the metadata catalog; the
    /// catalog's per-kind `match` is what guarantees exhaustiveness at compile time.
    pub const ALL: [WorkflowNodeKind; 37] = [
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
        WorkflowNodeKind::Cooldown,
        WorkflowNodeKind::AwaitRun,
        WorkflowNodeKind::Debounce,
        WorkflowNodeKind::Collect,
        WorkflowNodeKind::Barrier,
        WorkflowNodeKind::CircuitBreaker,
        WorkflowNodeKind::EventSource,
        WorkflowNodeKind::End,
        WorkflowNodeKind::Fail,
        WorkflowNodeKind::Interrupt,
        WorkflowNodeKind::Resume,
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
    /// unresolved expression naming this action's external effect, from `.idempotent(key: <expr>)`.
    /// the reducer resolves it against the run context at dispatch and stamps the result on the
    /// action command; the worker reserves that key before invoking the provider. `None` leaves the
    /// action non-idempotent, which is the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<Value>,
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
            #[serde(default)]
            pub idempotency_key: Option<Value>,
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
            idempotency_key: raw.idempotency_key,
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
    WorkflowObject::from_value(Value::Object(merged))
}
