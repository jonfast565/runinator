use runinator_models::value::Value;
use runinator_models::workflows::{WorkflowNode, WorkflowStatus};

use crate::compute::IntrinsicLibrary;
use crate::errors::WorkflowValidationError;
use crate::expressions::resolve_value_refs_with;
use crate::functions::EvalEnv;
use crate::keys::{
    COND_ALL, COND_ANY, COND_CONTAINS, COND_ENDS_WITH, COND_EQUALS, COND_EXISTS, COND_GREATER_THAN,
    COND_GREATER_THAN_OR_EQUAL, COND_IN, COND_LEFT, COND_LESS_THAN, COND_LESS_THAN_OR_EQUAL,
    COND_NOT, COND_NOT_EQUALS, COND_STARTS_WITH, COND_VALUE,
};

/// evaluate a condition in the eager reducer path: operands fold with the pure standard library, so
/// pure `$call` intrinsics work in declarative conditions. effectful intrinsics are not available
/// (the wdl front end rejects them outside compute blocks).
pub fn evaluate_condition(
    condition: &Value,
    context: &Value,
) -> Result<bool, WorkflowValidationError> {
    evaluate_condition_inner(
        condition,
        context,
        EvalEnv::lib_only(Some(&crate::compute::PureIntrinsics)),
    )
}

/// evaluate a condition whose operands may include `$call` intrinsics, resolved through `lib`.
pub fn evaluate_condition_with(
    condition: &Value,
    context: &Value,
    lib: &dyn IntrinsicLibrary,
) -> Result<bool, WorkflowValidationError> {
    evaluate_condition_inner(condition, context, EvalEnv::lib_only(Some(lib)))
}

/// evaluate a condition with a full evaluation environment (library + user functions). used by the
/// compute loop so conditions can call user-defined functions.
pub(crate) fn evaluate_condition_env(
    condition: &Value,
    context: &Value,
    env: EvalEnv,
) -> Result<bool, WorkflowValidationError> {
    evaluate_condition_inner(condition, context, env)
}

fn evaluate_condition_inner(
    condition: &Value,
    context: &Value,
    env: EvalEnv,
) -> Result<bool, WorkflowValidationError> {
    if condition.is_null() {
        return Ok(true);
    }
    let Some(object) = condition.as_object() else {
        return Err(WorkflowValidationError::InvalidCondition(
            "condition must be an object".into(),
        ));
    };
    let resolve = |value: &Value| resolve_value_refs_with(value, context, env);
    if let Some(all) = object.get(COND_ALL) {
        let Some(items) = all.as_array() else {
            return Err(WorkflowValidationError::InvalidCondition(
                "all must be an array".into(),
            ));
        };
        for item in items {
            if !evaluate_condition_inner(item, context, env)? {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    if let Some(any) = object.get(COND_ANY) {
        let Some(items) = any.as_array() else {
            return Err(WorkflowValidationError::InvalidCondition(
                "any must be an array".into(),
            ));
        };
        for item in items {
            if evaluate_condition_inner(item, context, env)? {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    if let Some(not) = object.get(COND_NOT) {
        return Ok(!evaluate_condition_inner(not, context, env)?);
    }

    let left = object
        .get(COND_VALUE)
        .or_else(|| object.get(COND_LEFT))
        .ok_or_else(|| WorkflowValidationError::InvalidCondition("missing value".into()))?;
    let left = resolve(left)?;
    if let Some(expected) = object.get(COND_EQUALS) {
        return Ok(left == resolve(expected)?);
    }
    if let Some(expected) = object.get(COND_NOT_EQUALS) {
        return Ok(left != resolve(expected)?);
    }
    if let Some(expected) = object.get(COND_CONTAINS) {
        return contains_value(&left, &resolve(expected)?);
    }
    if let Some(expected) = object.get(COND_IN) {
        return Ok(resolve(expected)?
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item == &left)));
    }
    if let Some(expected) = object.get(COND_STARTS_WITH) {
        return string_match(&left, &resolve(expected)?, |left, right| {
            left.starts_with(right)
        });
    }
    if let Some(expected) = object.get(COND_ENDS_WITH) {
        return string_match(&left, &resolve(expected)?, |left, right| {
            left.ends_with(right)
        });
    }
    if let Some(expected) = object.get(COND_GREATER_THAN) {
        return compare_value(&left, &resolve(expected)?, |ordering| ordering.is_gt());
    }
    if let Some(expected) = object.get(COND_GREATER_THAN_OR_EQUAL) {
        return compare_value(&left, &resolve(expected)?, |ordering| ordering.is_ge());
    }
    if let Some(expected) = object.get(COND_LESS_THAN) {
        return compare_value(&left, &resolve(expected)?, |ordering| ordering.is_lt());
    }
    if let Some(expected) = object.get(COND_LESS_THAN_OR_EQUAL) {
        return compare_value(&left, &resolve(expected)?, |ordering| ordering.is_le());
    }
    if let Some(expected) = object.get(COND_EXISTS) {
        return Ok(expected.as_bool().unwrap_or(true) != left.is_null());
    }
    if object.len() == 1 && object.contains_key(COND_VALUE) {
        return Ok(is_truthy(&left));
    }
    Err(WorkflowValidationError::InvalidCondition(
        "expected equals, not_equals, contains, in, starts_with, ends_with, greater_than, greater_than_or_equal, less_than, less_than_or_equal, exists, all, any, or not".into(),
    ))
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_i64().map_or_else(
            || {
                value.as_u64().map_or_else(
                    || {
                        value
                            .as_f64()
                            .is_some_and(|number| number != 0.0 && !number.is_nan())
                    },
                    |number| number != 0,
                )
            },
            |number| number != 0,
        ),
        Value::String(value) => !value.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn contains_value(left: &Value, expected: &Value) -> Result<bool, WorkflowValidationError> {
    if let (Some(text), Some(needle)) = (left.as_str(), expected.as_str()) {
        return Ok(text.contains(needle));
    }
    if let Some(items) = left.as_array() {
        return Ok(items.iter().any(|item| item == expected));
    }
    if let (Some(object), Some(key)) = (left.as_object(), expected.as_str()) {
        return Ok(object.contains_key(key));
    }
    Err(WorkflowValidationError::InvalidCondition(
        "contains requires a string, array, or object value".into(),
    ))
}

fn string_match(
    left: &Value,
    expected: &Value,
    predicate: impl FnOnce(&str, &str) -> bool,
) -> Result<bool, WorkflowValidationError> {
    let Some(text) = left.as_str() else {
        return Err(WorkflowValidationError::InvalidCondition(
            "string comparison requires a string value".into(),
        ));
    };
    let Some(expected) = expected.as_str() else {
        return Err(WorkflowValidationError::InvalidCondition(
            "string comparison requires a string operand".into(),
        ));
    };
    Ok(predicate(text, expected))
}

fn compare_value(
    left: &Value,
    expected: &Value,
    predicate: impl FnOnce(std::cmp::Ordering) -> bool,
) -> Result<bool, WorkflowValidationError> {
    if let (Some(left), Some(expected)) = (left.as_f64(), expected.as_f64()) {
        let Some(ordering) = left.partial_cmp(&expected) else {
            return Err(WorkflowValidationError::InvalidCondition(
                "numeric comparison is undefined".into(),
            ));
        };
        return Ok(predicate(ordering));
    }
    if let (Some(left), Some(expected)) = (left.as_str(), expected.as_str()) {
        return Ok(predicate(left.cmp(expected)));
    }
    Err(WorkflowValidationError::InvalidCondition(
        "ordering comparison requires both values to be numbers or strings".into(),
    ))
}

pub fn next_transition(
    node: &WorkflowNode,
    status: WorkflowStatus,
    context: &Value,
) -> Result<Option<String>, WorkflowValidationError> {
    // predicate edges are evaluated in ascending priority order; unset priorities sort last and a
    // stable sort preserves declaration order within a priority (and for all-unset branches).
    let mut ordered: Vec<&_> = node.transitions.branches.iter().collect();
    ordered.sort_by_key(|branch| branch.priority.unwrap_or(i64::MAX));
    for branch in ordered {
        if evaluate_condition(&branch.when, context)? {
            return Ok(Some(branch.target.as_str().to_string()));
        }
    }
    let target = match status {
        WorkflowStatus::Succeeded => node
            .transitions
            .on_success
            .as_ref()
            .or(node.transitions.next.as_ref()),
        WorkflowStatus::Failed | WorkflowStatus::Blocked => node.transitions.on_failure.as_ref(),
        WorkflowStatus::TimedOut => node.transitions.on_timeout.as_ref(),
        WorkflowStatus::Canceled => None,
        _ => node.transitions.next.as_ref(),
    };
    Ok(target.map(|target| target.as_str().to_string()))
}

pub fn validate_condition_value(condition: &Value) -> Result<(), WorkflowValidationError> {
    if condition.is_null() || condition.is_object() {
        Ok(())
    } else {
        Err(WorkflowValidationError::InvalidCondition(
            "condition must be an object".into(),
        ))
    }
}

pub(crate) fn validate_condition(condition: &Value) -> Result<(), WorkflowValidationError> {
    validate_condition_value(condition)
}
