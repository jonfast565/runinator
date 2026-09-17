use super::*;
use crate::functions::FunctionBinding;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum WorkflowWaitSeconds {
    Integer(i64),
    Expression(WorkflowObject),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatus {
    Queued,
    Running,
    Paused,
    DebugPaused,
    Waiting,
    Parked,
    Sleeping,
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
            WorkflowStatus::Parked => "parked",
            WorkflowStatus::Sleeping => "sleeping",
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
                | WorkflowStatus::Parked
                | WorkflowStatus::Sleeping
                | WorkflowStatus::ApprovalRequired
                | WorkflowStatus::InputRequired
                | WorkflowStatus::Blocked
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
            "parked" => Ok(WorkflowStatus::Parked),
            "sleeping" => Ok(WorkflowStatus::Sleeping),
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
    /// snapshot run state at a named point; enables rollback via the control-plane API.
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
    /// run a compiled invocation program, suspending on each durable call it makes.
    ///
    /// one node run spans every call the program yields on, which is what keeps retries, logs and
    /// artifacts attributed to the authored node rather than to a synthetic one per call.
    Invocation,
    /// begins an interrupt handler region. workflow metadata links a source to this entry.
    ///
    /// the interrupt analogue of [`WorkflowNodeKind::Start`]: a workflow has one primary entry
    /// point and one of these per declared handler. the runtime places a cursor here when it
    /// raises the interrupt; nothing may transition into it.
    Interrupt,
}

impl WorkflowNodeKind {
    /// every node kind in a stable, UI-facing order. used to build the metadata catalog; the
    /// catalog's per-kind `match` is what guarantees exhaustiveness at compile time.
    pub const ALL: [WorkflowNodeKind; 38] = [
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
        WorkflowNodeKind::Invocation,
        WorkflowNodeKind::End,
        WorkflowNodeKind::Fail,
        WorkflowNodeKind::Interrupt,
        WorkflowNodeKind::Resume,
    ];
}

fn default_timeout_seconds() -> i64 {
    60
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

mod workflow_wait;
pub use workflow_wait::WorkflowWait;

mod workflow_action;
pub use workflow_action::WorkflowAction;
