//! typed program ast for workflow expressions and compute blocks.
//!
//! these are the in-memory typed forms of the `$ref`/`$concat`/`$call`/`$if` expression encoding and
//! the `$let`/`$return`/`$goto`/`$if` compute-statement encoding. they live here (the lowest shared
//! crate) so `WorkflowNode` fields can be typed against them, while parsing, serializing, and
//! evaluation stay in `runinator-workflows`. the data here carries the *program*; runtime *data*
//! (inputs, outputs, run state) stays dynamic `Value`.

use crate::value::{Map, Value};
use crate::workflows::WorkflowNodeRef;

/// one segment of a value-reference path: an object key or an array index.
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
    const ORDER: [CompareOp; 10] = [
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

/// the typed form of a workflow condition: a boolean combinator tree over comparison leaves. leaf
/// operands stay dynamic `Value` because they are expressions resolved against the run context at
/// evaluation time (the expression tier), not static data.
#[derive(Debug, Clone, PartialEq)]
pub enum ConditionNode {
    All(Vec<ConditionNode>),
    Any(Vec<ConditionNode>),
    Not(Box<ConditionNode>),
    /// `{ value: <left>, <op>: <right> }`.
    Compare {
        left: Value,
        op: CompareOp,
        right: Value,
    },
    /// `{ value: <left>, exists: <bool> }`.
    Exists {
        left: Value,
        expected: bool,
    },
    /// `{ value: <left> }` — truthiness of the resolved left operand.
    Truthy {
        left: Value,
    },
    /// any object shape the evaluator does not recognize; carried verbatim so loading never fails
    /// and serialization is byte-identical. evaluating it yields the same error the evaluator does.
    Other(Value),
}

impl ConditionNode {
    // pull the leaf's left operand: `value` is preferred, `left` is the accepted alias.
    fn leaf_left(object: &Map) -> Option<&Value> {
        object.get(COND_VALUE).or_else(|| object.get(COND_LEFT))
    }
}

impl From<&ConditionNode> for Value {
    fn from(node: &ConditionNode) -> Self {
        match node {
            ConditionNode::All(items) => single(
                COND_ALL,
                Value::Array(items.iter().map(Value::from).collect()),
            ),
            ConditionNode::Any(items) => single(
                COND_ANY,
                Value::Array(items.iter().map(Value::from).collect()),
            ),
            ConditionNode::Not(inner) => single(COND_NOT, Value::from(inner.as_ref())),
            ConditionNode::Compare { left, op, right } => {
                let mut map = Map::new();
                map.insert(COND_VALUE.into(), left.clone());
                map.insert(op.key().into(), right.clone());
                Value::Object(map)
            }
            ConditionNode::Exists { left, expected } => {
                let mut map = Map::new();
                map.insert(COND_VALUE.into(), left.clone());
                map.insert(COND_EXISTS.into(), Value::Bool(*expected));
                Value::Object(map)
            }
            ConditionNode::Truthy { left } => single(COND_VALUE, left.clone()),
            ConditionNode::Other(value) => value.clone(),
        }
    }
}

impl From<ConditionNode> for Value {
    fn from(node: ConditionNode) -> Self {
        Value::from(&node)
    }
}

// parsing is total: any object the evaluator would reject is preserved as `Other` so loading never
// fails and byte-identity holds. the branch order mirrors `evaluate_condition_inner` exactly.
impl From<&Value> for ConditionNode {
    fn from(value: &Value) -> Self {
        let Some(object) = value.as_object() else {
            return ConditionNode::Other(value.clone());
        };
        if let Some(all) = object.get(COND_ALL) {
            return match all.as_array() {
                Some(items) => ConditionNode::All(items.iter().map(ConditionNode::from).collect()),
                None => ConditionNode::Other(value.clone()),
            };
        }
        if let Some(any) = object.get(COND_ANY) {
            return match any.as_array() {
                Some(items) => ConditionNode::Any(items.iter().map(ConditionNode::from).collect()),
                None => ConditionNode::Other(value.clone()),
            };
        }
        if let Some(not) = object.get(COND_NOT) {
            return ConditionNode::Not(Box::new(ConditionNode::from(not)));
        }
        let Some(left) = ConditionNode::leaf_left(object) else {
            return ConditionNode::Other(value.clone());
        };
        let left = left.clone();
        for op in CompareOp::ORDER {
            if let Some(right) = object.get(op.key()) {
                return ConditionNode::Compare {
                    left,
                    op,
                    right: right.clone(),
                };
            }
        }
        if let Some(exists) = object.get(COND_EXISTS) {
            return ConditionNode::Exists {
                left,
                expected: exists.as_bool().unwrap_or(true),
            };
        }
        if object.len() == 1 && object.contains_key(COND_VALUE) {
            return ConditionNode::Truthy { left };
        }
        ConditionNode::Other(value.clone())
    }
}

impl From<Value> for ConditionNode {
    fn from(value: Value) -> Self {
        ConditionNode::from(&value)
    }
}

fn single(key: &str, value: Value) -> Value {
    let mut map = Map::new();
    map.insert(key.to_string(), value);
    Value::Object(map)
}
