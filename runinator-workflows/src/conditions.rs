use runinator_models::workflows::{WorkflowNode, WorkflowStatus};
use serde_json::Value;

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
    if let Some(expected) = object.get("exists") {
        return Ok(expected.as_bool().unwrap_or(true) == !left.is_null());
    }
    Err(WorkflowValidationError::InvalidCondition(
        "expected equals, not_equals, exists, all, any, or not".into(),
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
