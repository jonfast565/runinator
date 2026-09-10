//! context behavior for the workflow interpreter.

use runinator_models::value::Value;
use runinator_models::workflow_vm::WorkflowContinuation;

pub(super) fn stable_id(namespace: uuid::Uuid, name: &str) -> uuid::Uuid {
    uuid::Uuid::new_v5(&namespace, name.as_bytes())
}

pub(super) fn local_context(continuation: &WorkflowContinuation) -> Value {
    let mut locals = continuation
        .locals
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<runinator_models::value::Map>();
    locals.insert(
        "__runinator_workspace_return".into(),
        continuation.stack.last().cloned().unwrap_or(Value::Null),
    );
    let mut context = locals.clone();
    let mut steps = context
        .get("steps")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for (name, output) in &continuation.locals {
        let Some(node_id) =
            name.strip_prefix(runinator_models::workflow_vm::WORKFLOW_NODE_OUTPUT_PREFIX)
        else {
            continue;
        };

        steps.insert(
            node_id.into(),
            runinator_models::json!({ "output": output }),
        );
    }
    context.insert("steps".into(), Value::Object(steps));
    // Compute bytecode reads `let` references with `LoadLocal`. The invocation VM seeds its entry
    // frame from this namespace so loop/map body values survive when effect arguments are frozen.
    context.insert("let".into(), Value::Object(locals));
    Value::Object(context)
}

/// Evaluate selectors that were deliberately kept explicit in bytecode so their ordering and
/// bucket calculation stay reproducible after authoring definitions have changed.
pub(super) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        _ => true,
    }
}
