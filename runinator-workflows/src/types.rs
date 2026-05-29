use runinator_models::workflows::WorkflowNodeRef;
use serde_json::Value;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowPathSegment {
    Key(String),
    Index(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowRefSource {
    Input,
    Prev,
    Workflow,
    NodeOutput(WorkflowNodeRef),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowValueRef {
    pub source: WorkflowRefSource,
    pub path: Vec<WorkflowPathSegment>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkflowExpression {
    Literal(Value),
    Ref(WorkflowValueRef),
    Concat(Vec<WorkflowExpression>),
    Coalesce(Vec<WorkflowExpression>),
    ToString(Box<WorkflowExpression>),
    ToJsonString(Box<WorkflowExpression>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchCase {
    pub target: WorkflowNodeRef,
    pub condition: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchParameters {
    pub value: Value,
    pub cases: Vec<SwitchCase>,
    pub default: Option<WorkflowNodeRef>,
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
    pub items: Value,
    pub target: WorkflowNodeRef,
    pub concurrency: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RaceParameters {
    pub branches: Vec<WorkflowNodeRef>,
    pub winner: BranchPolicy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmitParameters {
    pub event_type: Option<String>,
    pub data: Value,
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
pub struct LoopParameters {
    pub items: Vec<Value>,
}
