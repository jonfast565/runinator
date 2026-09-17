use super::*;

fn expand_local_defs_refs(value: &mut Value, stack: &mut Vec<String>) -> Result<(), String> {
    let defs = value
        .get("$defs")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    expand_refs_in_value(value, &defs, stack)
}

fn expand_refs_in_value(
    value: &mut Value,
    defs: &Value,
    stack: &mut Vec<String>,
) -> Result<(), String> {
    match value {
        Value::Object(map) => {
            if let Some(reference) = map.get("$ref").and_then(Value::as_str).map(str::to_string)
                && let Some(pointer) = reference.strip_prefix("#/$defs/")
            {
                if stack.iter().any(|item| item == &reference) {
                    return Err(format!("detected local $ref cycle for '{reference}'"));
                }
                let path = format!("/{pointer}");
                let mut replacement = defs
                    .pointer(&path)
                    .cloned()
                    .ok_or_else(|| format!("missing local $ref '{reference}'"))?;
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

fn merge_overlay(target: &mut Value, overlay: Value) {
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

fn deserialize_workflow_type<'de, D>(deserializer: D) -> Result<RuninatorType, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    serde_json::from_value(value.clone().into())
        .or_else(|_| Ok(RuninatorType::from_json_schema(&value)))
}

// note: raw json workflow bundles use an explicit client method because the server requires
// a risk-acknowledgment header before accepting them.

mod workflow_definition;
pub use workflow_definition::WorkflowDefinition;

mod workflow_graph;
pub use workflow_graph::WorkflowGraph;

mod workflow_duplicate_request;
pub use workflow_duplicate_request::WorkflowDuplicateRequest;

mod workflow_simulate_request;
pub use workflow_simulate_request::WorkflowSimulateRequest;

mod workflow_bundle;
pub use workflow_bundle::WorkflowBundle;
