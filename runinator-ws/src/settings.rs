// typed encoding and validation for the unified settings store. config values carry a json-schema
// (declared on the request, else inferred from the value on first write) that is pinned per
// (scope, name) and validated on every later write (hard error on mismatch); secrets are
// implicitly string-typed. these helpers are pure: callers pass the previously-stored schema
// (decoded from the persisted bytes) so this module never touches the database.

use runinator_models::settings::SettingKind;
use runinator_models::types::RuninatorType;
use runinator_models::value::Value;

use serde::{Deserialize, Serialize};

// the persisted form of a config entry: the json value plus the schema it was validated against,
// so the schema is pinned per (scope, name) and later value-only updates reuse it.
#[derive(Debug, Serialize, Deserialize)]
struct StoredConfig {
    value: Value,
    schema: Value,
}

/// decode a stored config payload back to its json value (back-compat: a bare value or string).
pub(crate) fn decode_config_value(bytes: &[u8]) -> Value {
    if let Ok(stored) = serde_json::from_slice::<StoredConfig>(bytes) {
        return stored.value;
    }
    serde_json::from_slice::<Value>(bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(bytes).into_owned()))
}

/// the schema pinned in a stored config payload, if it carries one.
pub(crate) fn decode_config_schema(bytes: &[u8]) -> Option<Value> {
    serde_json::from_slice::<StoredConfig>(bytes)
        .ok()
        .map(|stored| stored.schema)
}

/// the pinned type of a stored config slot, decoded from its bytes (back-compat: infer from the
/// bare value when no schema is stored).
pub(crate) fn stored_config_type(bytes: &[u8]) -> Option<RuninatorType> {
    match decode_config_schema(bytes) {
        Some(schema) => Some(RuninatorType::from_json_schema(&schema)),
        None => Some(RuninatorType::infer_from_value(&decode_config_value(bytes))),
    }
}

/// validate a value for its kind and produce the bytes to persist. config validates against a
/// schema (the request's, else `stored_schema` pinned on the first write, else one inferred from
/// the value on first write) and must conform to it; secrets must be a non-empty string.
/// `stored_schema` is the schema decoded from this slot's previously-stored bytes, if any.
pub(crate) fn validate_and_encode(
    kind: SettingKind,
    scope: &str,
    name: &str,
    value: &Value,
    schema: Option<&Value>,
    stored_schema: Option<&Value>,
) -> Result<Vec<u8>, String> {
    match kind {
        SettingKind::Secret => {
            let Value::String(text) = value else {
                return Err(format!(
                    "secret '{scope}/{name}' value must be a string, got {}",
                    value_type(value)
                ));
            };
            if text.trim().is_empty() {
                return Err(format!("secret '{scope}/{name}' value must not be empty"));
            }
            Ok(text.clone().into_bytes())
        }
        SettingKind::Config => {
            // a caller-supplied schema is checked as untrusted input; a stored schema (pinned on
            // the first write) is trusted; with neither, infer the schema from the value itself.
            let ty = match schema {
                Some(schema) => RuninatorType::from_json_schema_checked(schema)
                    .map_err(|err| format!("invalid config schema for '{scope}/{name}': {err}"))?,
                None => match stored_schema {
                    Some(stored) => RuninatorType::from_json_schema(stored),
                    None => RuninatorType::infer_from_value(value),
                },
            };
            ty.validate_value(value).map_err(|violation| {
                format!("config '{scope}/{name}' value does not match schema: {violation}")
            })?;
            serde_json::to_vec(&StoredConfig {
                value: value.clone(),
                schema: ty.to_json_schema(),
            })
            .map_err(|err| format!("failed to encode config '{scope}/{name}': {err}"))
        }
    }
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
