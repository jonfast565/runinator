use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowPathSegment {
    Key(String),
    Index(usize),
}

/// the root a value reference resolves against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowRefSource {
    Input,
    Prev,
    Workflow,
    Config,
    /// `interrupt.*` — what raised the interrupt this handler region is answering. only resolves
    /// inside a region; the root simply does not exist on an ordinary thread of control.
    Interrupt,
    // a compute-block local introduced by `let`, resolved from the `let` slot of the context.
    Local,
    NodeOutput(WorkflowNodeRef),
}

/// a resolved `$ref`: a source root plus a path into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowValueRef {
    pub source: WorkflowRefSource,
    pub path: Vec<WorkflowPathSegment>,
}

/// the typed form of a workflow expression (the `$ref`/`$concat`/`$call`/`$if`/... json encoding).
/// serializes through `Value` so it is field-ready with byte-identical json.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(into = "Value", try_from = "Value")]
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
    // application of an arbitrary callee value to arguments (`(obj.f)(x)`, `fns[0](x)`). the callee
    // evaluates to a first-class closure; a plain named call keeps the leaner `Call` form.
    Apply {
        callee: Box<WorkflowExpression>,
        args: Vec<WorkflowExpression>,
    },
    // a lazy conditional: the condition is evaluated, then only the taken branch is evaluated. this
    // laziness lets a recursive function's base case terminate before the recursive branch runs.
    Cond {
        condition: Box<WorkflowExpression>,
        then: Box<WorkflowExpression>,
        otherwise: Box<WorkflowExpression>,
    },
}

/// a single statement in a compute program.
#[derive(Debug, Clone, PartialEq)]
pub enum ComputeStmt {
    Let {
        name: String,
        value: WorkflowExpression,
    },
    Return(WorkflowExpression),
    Goto(String),
    If {
        condition: ConditionNode,
        then_branch: ComputeProgram,
        else_branch: ComputeProgram,
    },
    Expr(WorkflowExpression),
}

/// an ordered list of compute statements.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ComputeProgram(pub Vec<ComputeStmt>);

// condition encoding keys (the `{all|any|not}` combinator tree and the `{value, <op>}` leaf). kept
// here so both the typed ast conversions (this crate) and the workflows evaluator share one source.
pub const COND_ALL: &str = "all";
pub const COND_ANY: &str = "any";
pub const COND_NOT: &str = "not";
pub const COND_VALUE: &str = "value";
pub const COND_LEFT: &str = "left";
pub const COND_EXISTS: &str = "exists";

/// the binary comparison a condition leaf applies between its left and right operands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Equals,
    NotEquals,
    Contains,
    In,
    StartsWith,
    EndsWith,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
}

impl CompareOp {
    /// the wire key for this comparator (the object key that carries the right operand).
    pub fn key(self) -> &'static str {
        match self {
            CompareOp::Equals => "equals",
            CompareOp::NotEquals => "not_equals",
            CompareOp::Contains => "contains",
            CompareOp::In => "in",
            CompareOp::StartsWith => "starts_with",
            CompareOp::EndsWith => "ends_with",
            CompareOp::GreaterThan => "greater_than",
            CompareOp::GreaterThanOrEqual => "greater_than_or_equal",
            CompareOp::LessThan => "less_than",
            CompareOp::LessThanOrEqual => "less_than_or_equal",
        }
    }

    /// the comparator for a wire key, in the same precedence order the evaluator checks.
    pub fn from_key(key: &str) -> Option<CompareOp> {
        let op = match key {
            "equals" => CompareOp::Equals,
            "not_equals" => CompareOp::NotEquals,
            "contains" => CompareOp::Contains,
            "in" => CompareOp::In,
            "starts_with" => CompareOp::StartsWith,
            "ends_with" => CompareOp::EndsWith,
            "greater_than" => CompareOp::GreaterThan,
            "greater_than_or_equal" => CompareOp::GreaterThanOrEqual,
            "less_than" => CompareOp::LessThan,
            "less_than_or_equal" => CompareOp::LessThanOrEqual,
            _ => return None,
        };
        Some(op)
    }

    // the comparators in the exact order the evaluator probes them, so parsing is unambiguous.
    pub(super) const ORDER: [CompareOp; 10] = [
        CompareOp::Equals,
        CompareOp::NotEquals,
        CompareOp::Contains,
        CompareOp::In,
        CompareOp::StartsWith,
        CompareOp::EndsWith,
        CompareOp::GreaterThan,
        CompareOp::GreaterThanOrEqual,
        CompareOp::LessThan,
        CompareOp::LessThanOrEqual,
    ];
}
