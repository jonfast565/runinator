//! typed program ast for workflow expressions and compute blocks.
//!
//! these are the in-memory typed forms of the `$ref`/`$concat`/`$call`/`$if` expression encoding and
//! the `$let`/`$return`/`$goto`/`$if` compute-statement encoding. they live here (the lowest shared
//! crate) so `WorkflowNode` fields can be typed against them. the structural `Value` <-> ast parse and
//! serialize (validation preserved) live here too; only *evaluation* (ast + context -> value) stays in
//! `runinator-workflows`. the data here carries the *program*; runtime *data* (inputs, outputs, run
//! state) stays dynamic `Value`.

use std::fmt;

use serde::{Deserialize, Serialize};

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

// expression / ref / lambda wire keys. single source; `runinator-workflows` re-exports these.
pub const EXPR_VALUE: &str = "$value";
pub const EXPR_REF: &str = "$ref";
pub const EXPR_CONCAT: &str = "$concat";
pub const EXPR_COALESCE: &str = "$coalesce";
pub const EXPR_LITERAL: &str = "$literal";
pub const EXPR_TO_STRING: &str = "$to_string";
pub const EXPR_TO_JSON_STRING: &str = "$to_json_string";
pub const EXPR_NODE: &str = "$node";
pub const EXPR_ADD: &str = "$add";
pub const EXPR_SUB: &str = "$sub";
pub const EXPR_MUL: &str = "$mul";
pub const EXPR_DIV: &str = "$div";
pub const EXPR_MOD: &str = "$mod";
pub const EXPR_NEG: &str = "$neg";
pub const EXPR_CALL: &str = "$call";
pub const EXPR_ARGS: &str = "args";
pub const EXPR_LAMBDA: &str = "$lambda";
pub const LAMBDA_PARAMS: &str = "params";
pub const LAMBDA_BODY: &str = "body";
pub const EXPR_IF: &str = "$if";
pub const EXPR_THEN: &str = "then";
pub const EXPR_ELSE: &str = "else";
pub const REF_NODE: &str = "node";
pub const REF_OUTPUT: &str = "output";
pub const REF_PARAMS: &str = "params";
pub const REF_INPUT: &str = "input";
pub const REF_PREV: &str = "prev";
pub const REF_WORKFLOW: &str = "workflow";
pub const REF_CONFIG: &str = "config";
pub const REF_LOCAL: &str = "let";

/// a malformed workflow expression (rejected by the structural parser). the string is the offending
/// value's rendering, preserved for the error message the workflows layer surfaces.
#[derive(Debug, Clone, PartialEq)]
pub struct InvalidExpression(pub String);

impl fmt::Display for InvalidExpression {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid workflow expression: {}", self.0)
    }
}

impl std::error::Error for InvalidExpression {}

// -- value ref: structural parse/serialize ------------------------------------------------------

impl From<&WorkflowValueRef> for Value {
    fn from(reference: &WorkflowValueRef) -> Self {
        let path = Value::Array(
            reference
                .path
                .iter()
                .map(|segment| match segment {
                    WorkflowPathSegment::Key(key) => Value::String(key.clone()),
                    WorkflowPathSegment::Index(index) => Value::from(*index),
                })
                .collect(),
        );
        match &reference.source {
            WorkflowRefSource::Input => single(REF_PARAMS, path),
            WorkflowRefSource::Prev => single(REF_PREV, path),
            WorkflowRefSource::Workflow => single(REF_WORKFLOW, path),
            WorkflowRefSource::Config => single(REF_CONFIG, path),
            WorkflowRefSource::Local => single(REF_LOCAL, path),
            WorkflowRefSource::NodeOutput(node) => {
                let mut map = Map::new();
                map.insert(REF_NODE.into(), Value::String(node.as_str().to_string()));
                map.insert(REF_OUTPUT.into(), path);
                Value::Object(map)
            }
        }
    }
}

impl TryFrom<&Value> for WorkflowValueRef {
    type Error = InvalidExpression;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        let object = value.as_object().ok_or_else(|| invalid(value))?;
        if object.len() != 1
            && !(object.len() == 2
                && object.contains_key(REF_NODE)
                && object.contains_key(REF_OUTPUT))
        {
            return Err(invalid(value));
        }
        if let Some(path) = object.get(REF_PARAMS).or_else(|| object.get(REF_INPUT)) {
            return Ok(WorkflowValueRef {
                source: WorkflowRefSource::Input,
                path: parse_path(path)?,
            });
        }
        if let Some(path) = object.get(REF_PREV) {
            return Ok(WorkflowValueRef {
                source: WorkflowRefSource::Prev,
                path: parse_path(path)?,
            });
        }
        if let Some(path) = object.get(REF_WORKFLOW) {
            return Ok(WorkflowValueRef {
                source: WorkflowRefSource::Workflow,
                path: parse_path(path)?,
            });
        }
        if let Some(path) = object.get(REF_CONFIG) {
            return Ok(WorkflowValueRef {
                source: WorkflowRefSource::Config,
                path: parse_path(path)?,
            });
        }
        if let Some(path) = object.get(REF_LOCAL) {
            return Ok(WorkflowValueRef {
                source: WorkflowRefSource::Local,
                path: parse_path(path)?,
            });
        }
        if let (Some(node), Some(output)) = (object.get(REF_NODE), object.get(REF_OUTPUT)) {
            let node = node
                .as_str()
                .filter(|node| !node.is_empty())
                .ok_or_else(|| invalid(value))?;
            return Ok(WorkflowValueRef {
                source: WorkflowRefSource::NodeOutput(WorkflowNodeRef::new(node)),
                path: parse_path(output)?,
            });
        }
        Err(invalid(value))
    }
}

fn parse_path(value: &Value) -> Result<Vec<WorkflowPathSegment>, InvalidExpression> {
    let items = value.as_array().ok_or_else(|| invalid(value))?;
    items
        .iter()
        .map(|item| {
            if let Some(key) = item.as_str() {
                return Ok(WorkflowPathSegment::Key(key.to_string()));
            }
            if let Some(index) = item.as_u64() {
                return usize::try_from(index)
                    .map(WorkflowPathSegment::Index)
                    .map_err(|_| invalid(value));
            }
            Err(invalid(value))
        })
        .collect()
}

// -- expression: structural serialize -----------------------------------------------------------

impl From<&WorkflowExpression> for Value {
    fn from(expression: &WorkflowExpression) -> Self {
        match expression {
            WorkflowExpression::Literal(value) => value.clone(),
            WorkflowExpression::Ref(reference) => single(EXPR_REF, Value::from(reference)),
            WorkflowExpression::Concat(items) => single(EXPR_CONCAT, array(items)),
            WorkflowExpression::Coalesce(items) => single(EXPR_COALESCE, array(items)),
            WorkflowExpression::ToString(nested) => {
                single(EXPR_TO_STRING, Value::from(nested.as_ref()))
            }
            WorkflowExpression::ToJsonString(nested) => {
                single(EXPR_TO_JSON_STRING, Value::from(nested.as_ref()))
            }
            WorkflowExpression::Add(items) => single(EXPR_ADD, array(items)),
            WorkflowExpression::Sub(items) => single(EXPR_SUB, array(items)),
            WorkflowExpression::Mul(items) => single(EXPR_MUL, array(items)),
            WorkflowExpression::Div(items) => single(EXPR_DIV, array(items)),
            WorkflowExpression::Mod(items) => single(EXPR_MOD, array(items)),
            WorkflowExpression::Neg(nested) => single(EXPR_NEG, Value::from(nested.as_ref())),
            WorkflowExpression::Call { name, args } => {
                let mut map = Map::new();
                map.insert(EXPR_CALL.into(), Value::String(name.clone()));
                map.insert(EXPR_ARGS.into(), array(args));
                Value::Object(map)
            }
            WorkflowExpression::Lambda { params, body } => {
                let mut spec = Map::new();
                spec.insert(
                    LAMBDA_PARAMS.into(),
                    Value::Array(params.iter().map(|p| Value::String(p.clone())).collect()),
                );
                spec.insert(LAMBDA_BODY.into(), Value::from(body.as_ref()));
                single(EXPR_LAMBDA, Value::Object(spec))
            }
            WorkflowExpression::Cond {
                condition,
                then,
                otherwise,
            } => {
                let mut map = Map::new();
                map.insert(EXPR_IF.into(), Value::from(condition.as_ref()));
                map.insert(EXPR_THEN.into(), Value::from(then.as_ref()));
                map.insert(EXPR_ELSE.into(), Value::from(otherwise.as_ref()));
                Value::Object(map)
            }
        }
    }
}

impl From<WorkflowExpression> for Value {
    fn from(expression: WorkflowExpression) -> Self {
        Value::from(&expression)
    }
}

fn array(items: &[WorkflowExpression]) -> Value {
    Value::Array(items.iter().map(Value::from).collect())
}

// -- expression: structural parse (validating) --------------------------------------------------

impl TryFrom<&Value> for WorkflowExpression {
    type Error = InvalidExpression;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        match value {
            Value::Object(map) if map.contains_key(EXPR_VALUE) => Err(invalid(value)),
            Value::Object(map) if map.contains_key(EXPR_CALL) => {
                let name = map
                    .get(EXPR_CALL)
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid(value))?;
                if !map.keys().all(|key| key == EXPR_CALL || key == EXPR_ARGS) {
                    return Err(invalid(value));
                }
                let args = match map.get(EXPR_ARGS) {
                    None => Vec::new(),
                    Some(items) => items
                        .as_array()
                        .ok_or_else(|| invalid(value))?
                        .iter()
                        .map(WorkflowExpression::try_from)
                        .collect::<Result<Vec<_>, _>>()?,
                };
                Ok(WorkflowExpression::Call {
                    name: name.to_string(),
                    args,
                })
            }
            Value::Object(map) if map.contains_key(EXPR_LAMBDA) => {
                if map.len() != 1 {
                    return Err(invalid(value));
                }
                let spec = map
                    .get(EXPR_LAMBDA)
                    .and_then(Value::as_object)
                    .ok_or_else(|| invalid(value))?;
                let params = spec
                    .get(LAMBDA_PARAMS)
                    .and_then(Value::as_array)
                    .ok_or_else(|| invalid(value))?
                    .iter()
                    .map(|param| {
                        param
                            .as_str()
                            .map(str::to_string)
                            .ok_or_else(|| invalid(value))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let body = spec.get(LAMBDA_BODY).ok_or_else(|| invalid(value))?;
                Ok(WorkflowExpression::Lambda {
                    params,
                    body: Box::new(WorkflowExpression::try_from(body)?),
                })
            }
            Value::Object(map) if map.contains_key(EXPR_IF) => {
                if !map
                    .keys()
                    .all(|key| key == EXPR_IF || key == EXPR_THEN || key == EXPR_ELSE)
                {
                    return Err(invalid(value));
                }
                let branch = |key: &str| {
                    map.get(key)
                        .ok_or_else(|| invalid(value))
                        .and_then(WorkflowExpression::try_from)
                        .map(Box::new)
                };
                Ok(WorkflowExpression::Cond {
                    condition: branch(EXPR_IF)?,
                    then: branch(EXPR_THEN)?,
                    otherwise: branch(EXPR_ELSE)?,
                })
            }
            Value::Object(map)
                if map.contains_key(EXPR_REF)
                    || map.contains_key(EXPR_CONCAT)
                    || map.contains_key(EXPR_COALESCE)
                    || map.contains_key(EXPR_LITERAL)
                    || map.contains_key(EXPR_TO_STRING)
                    || map.contains_key(EXPR_TO_JSON_STRING)
                    || map.contains_key(EXPR_ADD)
                    || map.contains_key(EXPR_SUB)
                    || map.contains_key(EXPR_MUL)
                    || map.contains_key(EXPR_DIV)
                    || map.contains_key(EXPR_MOD)
                    || map.contains_key(EXPR_NEG)
                    || map.contains_key(EXPR_NODE) =>
            {
                if map.len() != 1 {
                    return Err(invalid(value));
                }
                if let Some(reference) = map.get(EXPR_REF) {
                    return Ok(WorkflowExpression::Ref(WorkflowValueRef::try_from(
                        reference,
                    )?));
                }
                for (key, ctor) in [
                    (
                        EXPR_ADD,
                        WorkflowExpression::Add
                            as fn(Vec<WorkflowExpression>) -> WorkflowExpression,
                    ),
                    (EXPR_SUB, WorkflowExpression::Sub),
                    (EXPR_MUL, WorkflowExpression::Mul),
                    (EXPR_DIV, WorkflowExpression::Div),
                    (EXPR_MOD, WorkflowExpression::Mod),
                ] {
                    if let Some(items) = map.get(key) {
                        let items = items
                            .as_array()
                            .filter(|items| !items.is_empty())
                            .ok_or_else(|| invalid(value))?;
                        return Ok(ctor(
                            items
                                .iter()
                                .map(WorkflowExpression::try_from)
                                .collect::<Result<Vec<_>, _>>()?,
                        ));
                    }
                }
                if let Some(operand) = map.get(EXPR_NEG) {
                    return Ok(WorkflowExpression::Neg(Box::new(
                        WorkflowExpression::try_from(operand)?,
                    )));
                }
                if let Some(items) = map.get(EXPR_CONCAT) {
                    let items = items.as_array().ok_or_else(|| invalid(value))?;
                    return Ok(WorkflowExpression::Concat(
                        items
                            .iter()
                            .map(WorkflowExpression::try_from)
                            .collect::<Result<Vec<_>, _>>()?,
                    ));
                }
                if let Some(items) = map.get(EXPR_COALESCE) {
                    let items = items
                        .as_array()
                        .filter(|items| !items.is_empty())
                        .ok_or_else(|| invalid(value))?;
                    return Ok(WorkflowExpression::Coalesce(
                        items
                            .iter()
                            .map(WorkflowExpression::try_from)
                            .collect::<Result<Vec<_>, _>>()?,
                    ));
                }
                if let Some(literal) = map.get(EXPR_LITERAL) {
                    return Ok(WorkflowExpression::Literal(literal.clone()));
                }
                if let Some(nested) = map.get(EXPR_TO_STRING) {
                    return Ok(WorkflowExpression::ToString(Box::new(
                        WorkflowExpression::try_from(nested)?,
                    )));
                }
                if let Some(nested) = map.get(EXPR_TO_JSON_STRING) {
                    return Ok(WorkflowExpression::ToJsonString(Box::new(
                        WorkflowExpression::try_from(nested)?,
                    )));
                }
                Err(invalid(value))
            }
            Value::Object(map) => {
                let mut resolved = Map::new();
                for (key, nested) in map {
                    resolved.insert(
                        key.clone(),
                        Value::from(&WorkflowExpression::try_from(nested)?),
                    );
                }
                Ok(WorkflowExpression::Literal(Value::Object(resolved)))
            }
            Value::Array(items) => Ok(WorkflowExpression::Literal(Value::Array(
                items
                    .iter()
                    .map(|item| Ok(Value::from(&WorkflowExpression::try_from(item)?)))
                    .collect::<Result<Vec<_>, InvalidExpression>>()?,
            ))),
            Value::String(raw) if raw.contains("{{") || raw.contains("}}") => {
                Err(InvalidExpression(raw.clone()))
            }
            _ => Ok(WorkflowExpression::Literal(value.clone())),
        }
    }
}

impl TryFrom<Value> for WorkflowExpression {
    type Error = InvalidExpression;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        WorkflowExpression::try_from(&value)
    }
}

fn invalid(value: &Value) -> InvalidExpression {
    InvalidExpression(value.to_string())
}

// compute statement wire keys (`$if`/`then`/`else` are shared with the expression `Cond` form).
pub const STMT_LET: &str = "$let";
pub const STMT_VALUE: &str = "value";
pub const STMT_RETURN: &str = "$return";
pub const STMT_GOTO: &str = "$goto";

// -- compute program: structural serialize (inverse of `parse_program`) -------------------------

impl From<&ComputeStmt> for Value {
    fn from(statement: &ComputeStmt) -> Self {
        match statement {
            ComputeStmt::Let { name, value } => {
                let mut map = Map::new();
                map.insert(STMT_LET.into(), Value::String(name.clone()));
                map.insert(STMT_VALUE.into(), Value::from(value));
                Value::Object(map)
            }
            ComputeStmt::Return(expr) => single(STMT_RETURN, Value::from(expr)),
            ComputeStmt::Goto(target) => single(STMT_GOTO, Value::String(target.clone())),
            ComputeStmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let mut map = Map::new();
                map.insert(EXPR_IF.into(), Value::from(condition));
                map.insert(EXPR_THEN.into(), Value::from(then_branch));
                map.insert(EXPR_ELSE.into(), Value::from(else_branch));
                Value::Object(map)
            }
            // a bare expression statement serializes as the expression itself.
            ComputeStmt::Expr(expr) => Value::from(expr),
        }
    }
}

impl From<&ComputeProgram> for Value {
    fn from(program: &ComputeProgram) -> Self {
        Value::Array(program.0.iter().map(Value::from).collect())
    }
}

fn single(key: &str, value: Value) -> Value {
    let mut map = Map::new();
    map.insert(key.to_string(), value);
    Value::Object(map)
}
