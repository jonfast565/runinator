use runinator_models::orchestration::GateKind;
use runinator_models::value::Value;
use runinator_models::workflow_ast::WorkflowExpression;
use runinator_models::workflows::{WorkflowCondition, WorkflowNodeRef};

// the expression/ref/compute program ast now lives in `runinator_models::workflow_ast` so
// `WorkflowNode` fields can be typed against it; this module keeps the per-node parameter structs.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchPolicy {
    All,
    Any,
    FirstSuccess,
}

impl BranchPolicy {
    pub fn parse(value: Option<&Value>, default: BranchPolicy) -> Result<Self, String> {
        match value.and_then(Value::as_str) {
            None => Ok(default),
            Some("all") => Ok(BranchPolicy::All),
            Some("any") => Ok(BranchPolicy::Any),
            Some("first_success") => Ok(BranchPolicy::FirstSuccess),
            Some(other) => Err(format!("unsupported branch policy '{other}'")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchCase {
    pub target: WorkflowNodeRef,
    pub condition: WorkflowCondition,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchParameters {
    pub value: WorkflowExpression,
    pub cases: Vec<SwitchCase>,
    pub default: Option<WorkflowNodeRef>,
}

/// a literal light switch: `value` truthiness routes to `on`, otherwise `off`.
#[derive(Debug, Clone, PartialEq)]
pub struct ToggleParameters {
    pub value: WorkflowExpression,
    pub on: WorkflowNodeRef,
    pub off: WorkflowNodeRef,
}

/// a weighted, hash-bucketed router: `hash(key) % total_weight` selects a bucket. sticky per key.
#[derive(Debug, Clone, PartialEq)]
pub struct PercentageParameters {
    pub key: WorkflowExpression,
    pub buckets: Vec<PercentageBucket>,
    pub default: Option<WorkflowNodeRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PercentageBucket {
    pub weight: i64,
    pub target: WorkflowNodeRef,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParallelParameters {
    pub branches: Vec<WorkflowNodeRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JoinParameters {
    pub wait_for: Vec<WorkflowNodeRef>,
    pub mode: BranchPolicy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TryParameters {
    pub body: WorkflowNodeRef,
    pub catch: Option<WorkflowNodeRef>,
    pub finally: Option<WorkflowNodeRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MapParameters {
    pub items: WorkflowExpression,
    pub target: WorkflowNodeRef,
    pub concurrency: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RaceParameters {
    pub branches: Vec<WorkflowNodeRef>,
    pub winner: BranchPolicy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutputParameters {
    pub event_type: Option<String>,
    pub data: WorkflowExpression,
    /// artifact declarations: name/source pairs promoted to run-level by this output node.
    pub items: Vec<ArtifactItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArtifactItem {
    pub name: String,
    /// value-ref that resolves to an artifact descriptor (or array of them) at runtime.
    pub source: WorkflowExpression,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InputParameters {
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WaitParameters {
    pub seconds: i64,
    pub until_status: Option<String>,
    pub initial_status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalParameters {
    pub approval_type: String,
    pub prompt: String,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SignalParameters {
    pub name: String,
    /// unresolved correlation-key expression (often a ref); the reducer resolves it at park time.
    pub correlation_key: WorkflowExpression,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GateParameters {
    pub kind: GateKind,
    pub condition: WorkflowCondition,
    pub poll_interval_seconds: i64,
    pub deadline_seconds: Option<i64>,
    pub label: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoopParameters {
    pub items: Vec<Value>,
}
