//! Deferred references into the attached snapshot's named results.
use runinator_models::{
    errors::{SendableError, WORKSPACE_INVALID},
    value::Value,
};
use std::collections::BTreeMap;

pub fn resolve_results(
    input: &Value,
    results: Option<&BTreeMap<String, Value>>,
) -> Result<Value, SendableError> {
    let mut input: serde_json::Value = input.clone().into();
    let results = results.map(serde_json::to_value).transpose()?;
    resolve(&mut input, results.as_ref())?;
    Ok(input.into())
}

fn resolve(
    value: &mut serde_json::Value,
    results: Option<&serde_json::Value>,
) -> Result<(), SendableError> {
    match value {
        serde_json::Value::Object(object) if object.contains_key("$workspace") => {
            let pointer = object
                .get("$workspace")
                .and_then(serde_json::Value::as_str)
                .filter(|_| object.len() == 1)
                .ok_or_else(|| {
                    WORKSPACE_INVALID
                        .error("workspace result reference must contain only a JSON pointer")
                })?;
            *value = results
                .and_then(|results| results.pointer(pointer))
                .cloned()
                .ok_or_else(|| {
                    WORKSPACE_INVALID.error(format!(
                        "workspace result reference '{pointer}' could not be resolved"
                    ))
                })?;
        }
        serde_json::Value::Object(object) => {
            for value in object.values_mut() {
                resolve(value, results)?;
            }
        }
        serde_json::Value::Array(items) => {
            for value in items {
                resolve(value, results)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
#[path = "results_tests.rs"]
mod tests;
