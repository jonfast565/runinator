use runinator_models::value::{Map, Value};
use runinator_models::workflows::WorkflowDefinition;

use crate::errors::WorkflowValidationError;

pub fn expand_workflow_refs(
    workflow: &WorkflowDefinition,
) -> Result<Value, WorkflowValidationError> {
    let mut root = workflow.definition.clone();
    let defs = root
        .get("$defs")
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    let mut stack = Vec::new();
    expand_refs_in_value(&mut root, &defs, &mut stack)?;
    Ok(root)
}

pub(crate) fn expand_refs_in_value(
    value: &mut Value,
    defs: &Value,
    stack: &mut Vec<String>,
) -> Result<(), WorkflowValidationError> {
    match value {
        Value::Object(map) => {
            if let Some(reference) = map.get("$ref").and_then(Value::as_str).map(str::to_string) {
                if let Some(pointer) = reference.strip_prefix("#/$defs/") {
                    if stack.iter().any(|item| item == &reference) {
                        return Err(WorkflowValidationError::RefCycle(reference));
                    }
                    let path = format!("/{pointer}");
                    let mut replacement = defs
                        .pointer(&path)
                        .cloned()
                        .ok_or_else(|| WorkflowValidationError::MissingRef(reference.clone()))?;
                    stack.push(reference.clone());
                    expand_refs_in_value(&mut replacement, defs, stack)?;
                    stack.pop();
                    for (key, overlay) in map.clone() {
                        if key != "$ref"
                            && key != "with"
                            && let Value::Object(replacement_map) = &mut replacement
                        {
                            replacement_map.insert(key, overlay);
                        }
                    }
                    if let Some(with) = map.get("with") {
                        merge_overlay(&mut replacement, with.clone());
                    }
                    *value = replacement;
                    return Ok(());
                }
                if reference.starts_with("runinator://") {
                    return Ok(());
                }
                return Err(WorkflowValidationError::MissingRef(reference));
            }
            for nested in map.values_mut() {
                expand_refs_in_value(nested, defs, stack)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                expand_refs_in_value(item, defs, stack)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn merge_overlay(target: &mut Value, overlay: Value) {
    match (target, overlay) {
        (Value::Object(target), Value::Object(overlay)) => {
            for (key, value) in overlay {
                match target.get_mut(&key) {
                    Some(existing) => merge_overlay(existing, value),
                    None => {
                        target.insert(key, value);
                    }
                }
            }
        }
        (target, overlay) => *target = overlay,
    }
}
