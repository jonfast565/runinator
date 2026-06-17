use runinator_models::orchestration::GateKind;
use runinator_models::value::Value;
use runinator_models::workflows::WorkflowNodeRef;

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
    Config,
    // a compute-block local introduced by `let`, resolved from the `let` slot of the context.
    Local,
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
    // arithmetic ops fold their operands left-to-right; require at least one operand.
    Add(Vec<WorkflowExpression>),
    Sub(Vec<WorkflowExpression>),
    Mul(Vec<WorkflowExpression>),
    Div(Vec<WorkflowExpression>),
    Mod(Vec<WorkflowExpression>),
    Neg(Box<WorkflowExpression>),
    // a call into the intrinsic library, resolved by name at evaluation time.
    Call {
        name: String,
        args: Vec<WorkflowExpression>,
    },
    // an anonymous function passed to a higher-order intrinsic (map/filter/reduce/...). its body is
    // evaluated per element with the params bound into the `let` slot; it has no standalone value.
    Lambda {
        params: Vec<String>,
        body: Box<WorkflowExpression>,
    },
    // a lazy conditional: the condition is evaluated, then only the taken branch is evaluated. this
    // laziness lets a recursive function's base case terminate before the recursive branch runs.
    Cond {
        condition: Box<WorkflowExpression>,
        then: Box<WorkflowExpression>,
        otherwise: Box<WorkflowExpression>,
    },
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
pub struct OutputParameters {
    pub event_type: Option<String>,
    pub data: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeliverableParameters {
    pub items: Vec<DeliverableItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeliverableItem {
    pub name: String,
    /// value-ref that resolves to an artifact descriptor (or array of them) at runtime.
    pub source: Value,
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
    /// unresolved correlation-key value (often a ref); the reducer resolves it at park time.
    pub correlation_key: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GateParameters {
    pub kind: GateKind,
    pub condition: Value,
    pub poll_interval_seconds: i64,
    pub deadline_seconds: Option<i64>,
    pub label: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoopParameters {
    pub items: Vec<Value>,
}
