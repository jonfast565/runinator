use runinator_models::value::Value;
use runinator_models::workflow_ast::ConditionNode;
use runinator_models::workflows::{WorkflowCondition, WorkflowNode, WorkflowStatus};

use crate::assemble::assemble_condition;
use crate::catalog::CallableCatalog;
use crate::errors::WorkflowValidationError;
use crate::vm::evaluate_module_pure;

/// Evaluate a declarative condition through a synchronous VM module.
pub fn evaluate_condition(
    condition: &Value,
    context: &Value,
) -> Result<bool, WorkflowValidationError> {
    if condition.is_null() {
        return Ok(true);
    }
    if !condition.is_object() {
        return Err(WorkflowValidationError::InvalidCondition(
            "condition must be an object".into(),
        ));
    }
    evaluate_node(&ConditionNode::from(condition), context)
}

/// Evaluate the typed condition carried by a workflow edge.
pub fn evaluate_workflow_condition(
    condition: &WorkflowCondition,
    context: &Value,
) -> Result<bool, WorkflowValidationError> {
    match condition.node() {
        None => Ok(true),
        Some(node) => evaluate_node(node, context),
    }
}

fn evaluate_node(node: &ConditionNode, context: &Value) -> Result<bool, WorkflowValidationError> {
    let catalog = CallableCatalog::builtin();
    let program = assemble_condition(node, &catalog).map_err(as_condition_error)?;
    let value = evaluate_module_pure(
        &runinator_models::invocation::InvocationModule::new(program),
        context,
        &catalog,
    )
    .map_err(as_condition_error)?;
    value.as_bool().ok_or_else(|| {
        WorkflowValidationError::InvalidCondition(
            "condition program did not return a boolean".into(),
        )
    })
}

fn as_condition_error(error: WorkflowValidationError) -> WorkflowValidationError {
    match error {
        WorkflowValidationError::InvalidCondition(_) => error,
        other => WorkflowValidationError::InvalidCondition(other.to_string()),
    }
}

/// Shared truthiness used by condition bytecode and conditional expressions.
pub(crate) fn is_truthy(value: &Value) -> bool {
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

pub fn next_transition(
    node: &WorkflowNode,
    status: WorkflowStatus,
    context: &Value,
) -> Result<Option<String>, WorkflowValidationError> {
    let mut ordered: Vec<&_> = node.transitions.branches.iter().collect();
    ordered.sort_by_key(|branch| branch.priority.unwrap_or(i64::MAX));
    for branch in ordered {
        if evaluate_workflow_condition(&branch.when, context)? {
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

pub fn validate_condition(condition: &Value) -> Result<(), WorkflowValidationError> {
    validate_condition_value(condition)
}
