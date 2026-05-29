use runinator_models::value::Value;
use runinator_models::workflows::{WorkflowNode, WorkflowStatus};

use crate::errors::WorkflowValidationError;
use crate::expressions::resolve_value_refs;

pub fn evaluate_condition(
    condition: &Value,
    context: &Value,
) -> Result<bool, WorkflowValidationError> {
    if condition.is_null() {
        return Ok(true);
    }
    let Some(object) = condition.as_object() else {
        return Err(WorkflowValidationError::InvalidCondition(
            "condition must be an object".into(),
        ));
    };
    if let Some(all) = object.get("all") {
        let Some(items) = all.as_array() else {
            return Err(WorkflowValidationError::InvalidCondition(
                "all must be an array".into(),
            ));
        };
        for item in items {
            if !evaluate_condition(item, context)? {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    if let Some(any) = object.get("any") {
        let Some(items) = any.as_array() else {
            return Err(WorkflowValidationError::InvalidCondition(
                "any must be an array".into(),
            ));
        };
        for item in items {
            if evaluate_condition(item, context)? {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    if let Some(not) = object.get("not") {
        return Ok(!evaluate_condition(not, context)?);
    }

    let left = object
        .get("value")
        .or_else(|| object.get("left"))
        .ok_or_else(|| WorkflowValidationError::InvalidCondition("missing value".into()))?;
    let left = resolve_value_refs(left, context)?;
    if let Some(expected) = object.get("equals") {
        return Ok(left == resolve_value_refs(expected, context)?);
    }
    if let Some(expected) = object.get("not_equals") {
        return Ok(left != resolve_value_refs(expected, context)?);
    }
    if let Some(expected) = object.get("contains") {
        return contains_value(&left, &resolve_value_refs(expected, context)?);
    }
    if let Some(expected) = object.get("in") {
        return Ok(resolve_value_refs(expected, context)?
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item == &left)));
    }
    if let Some(expected) = object.get("starts_with") {
        return string_match(
            &left,
            &resolve_value_refs(expected, context)?,
            |left, right| left.starts_with(right),
        );
    }
    if let Some(expected) = object.get("ends_with") {
        return string_match(
            &left,
            &resolve_value_refs(expected, context)?,
            |left, right| left.ends_with(right),
        );
    }
    if let Some(expected) = object.get("greater_than") {
        return compare_value(&left, &resolve_value_refs(expected, context)?, |ordering| {
            ordering.is_gt()
        });
    }
    if let Some(expected) = object.get("greater_than_or_equal") {
        return compare_value(&left, &resolve_value_refs(expected, context)?, |ordering| {
            ordering.is_ge()
        });
    }
    if let Some(expected) = object.get("less_than") {
        return compare_value(&left, &resolve_value_refs(expected, context)?, |ordering| {
            ordering.is_lt()
        });
    }
    if let Some(expected) = object.get("less_than_or_equal") {
        return compare_value(&left, &resolve_value_refs(expected, context)?, |ordering| {
            ordering.is_le()
        });
    }
    if let Some(expected) = object.get("exists") {
        return Ok(expected.as_bool().unwrap_or(true) != left.is_null());
    }
    Err(WorkflowValidationError::InvalidCondition(
        "expected equals, not_equals, contains, in, starts_with, ends_with, greater_than, greater_than_or_equal, less_than, less_than_or_equal, exists, all, any, or not".into(),
    ))
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
    for branch in &node.transitions.branches {
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

pub(crate) fn validate_condition(condition: &Value) -> Result<(), WorkflowValidationError> {
    if condition.is_null() || condition.is_object() {
        Ok(())
    } else {
        Err(WorkflowValidationError::InvalidCondition(
            "condition must be an object".into(),
        ))
    }
}
