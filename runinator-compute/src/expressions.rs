use runinator_models::types::RuninatorType;
use runinator_models::value::{Map, Value};
use runinator_models::workflow_ast::{
    WorkflowExpression, WorkflowPathSegment, WorkflowRefSource, WorkflowValueRef,
};

use crate::assemble::assemble_expression;
use crate::catalog::CallableCatalog;
use crate::errors::WorkflowValidationError;
use crate::functions::FunctionTable;
use crate::keys::{
    REF_CONFIG, REF_INPUT, REF_INTERRUPT, REF_LOCAL, REF_OUTPUT, REF_PREV, REF_STEPS, REF_WORKFLOW,
};
use crate::vm::evaluate_module_pure;

/// Resolve a declarative value through a synchronous VM module.
pub fn resolve_value_refs(
    value: &Value,
    context: &Value,
) -> Result<Value, WorkflowValidationError> {
    let expression = parse_expression(value)?;
    evaluate_expression(&expression, context)
}

/// Pure reducer/preview form of [`resolve_value_refs`].
pub fn resolve_value_refs_pure(
    value: &Value,
    context: &Value,
) -> Result<Value, WorkflowValidationError> {
    resolve_value_refs(value, context)
}

/// Validate a value without resolving it.
pub fn validate_expression(value: &Value) -> Result<(), WorkflowValidationError> {
    parse_expression(value).map(|_| ())
}

/// Evaluate a parsed expression through the VM.
pub fn evaluate_expression(
    expression: &WorkflowExpression,
    context: &Value,
) -> Result<Value, WorkflowValidationError> {
    let catalog = CallableCatalog::builtin();
    let program = assemble_expression(expression, &catalog).map_err(as_value_error)?;
    evaluate_module_pure(
        &runinator_models::invocation::InvocationModule::new(program),
        context,
        &catalog,
    )
    .map_err(as_value_error)
}

/// Evaluate an expression with workflow-defined functions included in the same module.
pub fn resolve_value_refs_with_functions(
    value: &Value,
    context: &Value,
    functions: &FunctionTable,
) -> Result<Value, WorkflowValidationError> {
    let expression = parse_expression(value)?;
    let catalog = functions.catalog();
    let module = functions
        .module_for_expression(&expression)
        .map_err(as_value_error)?;
    evaluate_module_pure(&module, context, &catalog).map_err(as_value_error)
}

/// Fill omitted top-level input fields from their declared defaults. Defaults intentionally run
/// repeatedly against the same mutable context so sibling defaults retain their existing ordering.
pub fn apply_input_defaults(context: &mut Value, input_type: &RuninatorType) {
    let RuninatorType::Struct { fields, .. } = input_type else {
        return;
    };
    if fields.values().all(|field| field.default.is_none()) {
        return;
    }
    let needs_object = match context.get(REF_INPUT) {
        Some(value) => value.is_null(),
        None => true,
    };
    if needs_object && let Some(object) = context.as_object_mut() {
        object.insert(REF_INPUT.into(), Value::Object(Map::new()));
    }
    if !context.get(REF_INPUT).is_some_and(Value::is_object) {
        return;
    }
    loop {
        let mut progressed = false;
        for (name, field) in fields {
            let Some(default) = &field.default else {
                continue;
            };
            if context
                .get(REF_INPUT)
                .and_then(|input| input.get(name))
                .is_some()
            {
                continue;
            }
            let Ok(value) = resolve_value_refs(default, context) else {
                continue;
            };
            if let Some(input) = context.get_mut(REF_INPUT).and_then(Value::as_object_mut) {
                input.insert(name.clone(), value);
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
}

pub fn parse_expression(value: &Value) -> Result<WorkflowExpression, WorkflowValidationError> {
    WorkflowExpression::try_from(value)
        .map_err(|err| WorkflowValidationError::InvalidValueRef(err.0))
}

fn as_value_error(error: WorkflowValidationError) -> WorkflowValidationError {
    match error {
        WorkflowValidationError::InvalidValueRef(_) => error,
        other => WorkflowValidationError::InvalidValueRef(other.to_string()),
    }
}

pub fn parse_value_ref(value: &Value) -> Result<WorkflowValueRef, WorkflowValidationError> {
    WorkflowValueRef::try_from(value).map_err(|err| WorkflowValidationError::InvalidValueRef(err.0))
}

pub(crate) fn resolve_value_ref(
    reference: &WorkflowValueRef,
    context: &Value,
) -> Result<Value, WorkflowValidationError> {
    if let WorkflowRefSource::NodeOutput(node) = &reference.source {
        let step = context
            .get(REF_STEPS)
            .and_then(|steps| steps.get(node.as_str()))
            .ok_or_else(|| {
                WorkflowValidationError::InvalidValueRef(serialize_value_ref(reference).to_string())
            })?;
        let from_output = step
            .get(REF_OUTPUT)
            .and_then(|output| resolve_path(output, &reference.path));
        return Ok(match from_output {
            Some(value) => value.clone(),
            None => resolve_path(step, &reference.path)
                .cloned()
                .unwrap_or(Value::Null),
        });
    }
    let base = match &reference.source {
        WorkflowRefSource::Input => context.get(REF_INPUT),
        WorkflowRefSource::Prev => context.get(REF_PREV),
        WorkflowRefSource::Workflow => context.get(REF_WORKFLOW),
        WorkflowRefSource::Config => context.get(REF_CONFIG),
        WorkflowRefSource::Interrupt => context.get(REF_INTERRUPT),
        WorkflowRefSource::Local => context.get(REF_LOCAL),
        WorkflowRefSource::NodeOutput(_) => None,
    }
    .ok_or_else(|| {
        WorkflowValidationError::InvalidValueRef(serialize_value_ref(reference).to_string())
    })?;
    Ok(resolve_path(base, &reference.path)
        .cloned()
        .unwrap_or(Value::Null))
}

pub(crate) fn resolve_path<'a>(
    value: &'a Value,
    path: &[WorkflowPathSegment],
) -> Option<&'a Value> {
    path.iter()
        .try_fold(value, |current, segment| match segment {
            WorkflowPathSegment::Key(key) => current.get(key),
            WorkflowPathSegment::Index(index) => current.get(*index),
        })
}

pub fn serialize_value_ref(reference: &WorkflowValueRef) -> Value {
    Value::from(reference)
}

#[cfg(test)]
mod tests {
    use runinator_models::json;

    use super::*;

    #[test]
    fn resolves_references_inside_literal_arrays_and_objects() {
        let value = json!({
            "rows": [
                { "id": { "$ref": { "input": ["id"] } } },
                { "$call": "add", "args": [1, 2] }
            ]
        });
        assert_eq!(
            resolve_value_refs(&value, &json!({ "input": { "id": "A-1" } })).unwrap(),
            json!({ "rows": [{ "id": "A-1" }, 3] })
        );
    }

    #[test]
    fn declarative_calls_cannot_dispatch() {
        let error = resolve_value_refs(
            &json!({ "$call": "http_get", "args": ["https://example.test"] }),
            &Value::Null,
        )
        .expect_err("durable calls must yield rather than run in a reducer");
        assert!(matches!(error, WorkflowValidationError::InvalidValueRef(_)));
    }
}
