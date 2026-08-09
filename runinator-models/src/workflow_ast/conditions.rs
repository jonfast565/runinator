use super::conversions::single;
use super::*;

/// the typed form of a workflow condition: a boolean combinator tree over comparison leaves.
#[derive(Debug, Clone, PartialEq)]
pub enum ConditionNode {
    All(Vec<ConditionNode>),
    Any(Vec<ConditionNode>),
    Not(Box<ConditionNode>),
    /// `{ value: <left>, <op>: <right> }`.
    Compare {
        left: WorkflowExpression,
        op: CompareOp,
        right: WorkflowExpression,
    },
    /// `{ value: <left>, exists: <bool> }`.
    Exists {
        left: WorkflowExpression,
        expected: bool,
    },
    /// `{ value: <left> }` — truthiness of the resolved left operand.
    Truthy {
        left: WorkflowExpression,
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
                map.insert(COND_VALUE.into(), Value::from(left));
                map.insert(op.key().into(), Value::from(right));
                Value::Object(map)
            }
            ConditionNode::Exists { left, expected } => {
                let mut map = Map::new();
                map.insert(COND_VALUE.into(), Value::from(left));
                map.insert(COND_EXISTS.into(), Value::Bool(*expected));
                Value::Object(map)
            }
            ConditionNode::Truthy { left } => single(COND_VALUE, Value::from(left)),
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
        // the leaf operands belong to the expression tier; an operand that does not parse as an
        // expression means the whole leaf is preserved verbatim as `Other`, keeping `From` total.
        let Ok(left) = WorkflowExpression::try_from(left) else {
            return ConditionNode::Other(value.clone());
        };
        for op in CompareOp::ORDER {
            if let Some(right) = object.get(op.key()) {
                let Ok(right) = WorkflowExpression::try_from(right) else {
                    return ConditionNode::Other(value.clone());
                };
                return ConditionNode::Compare { left, op, right };
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
pub const EXPR_APPLY: &str = "$apply";
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
/// the root an interrupt handler region reads what raised it under.
pub const REF_INTERRUPT: &str = "interrupt";
pub const REF_LOCAL: &str = "let";
