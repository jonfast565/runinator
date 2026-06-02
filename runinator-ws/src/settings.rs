// typed encoding and validation for the unified settings store. config values carry a declared
// json-schema, validated on every write (hard error on mismatch or when no schema is known);
// secrets are implicitly string-typed.

use runinator_models::settings::SettingKind;
use runinator_models::types::RuninatorType;
use runinator_models::value::Value;
use runinator_utilities::credential_store::CredentialStore;

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

// the schema previously stored for a config slot, if any.
fn stored_config_schema(store: &dyn CredentialStore, scope: &str, name: &str) -> Option<Value> {
    let bytes = store.get(SettingKind::Config, scope, name).ok()??;
    serde_json::from_slice::<StoredConfig>(&bytes)
        .ok()
        .map(|stored| stored.schema)
}

/// validate a value for its kind and produce the bytes to persist. config requires a declared
/// schema (from the request, else the previously stored one) and must conform to it; secrets must
/// be a non-empty string. returns a human-readable error on any violation.
pub(crate) fn validate_and_encode(
    store: &dyn CredentialStore,
    kind: SettingKind,
    scope: &str,
    name: &str,
    value: &Value,
    schema: Option<&Value>,
) -> Result<Vec<u8>, String> {
    match kind {
        SettingKind::Secret => {
            let Value::String(text) = value else {
                return Err(format!(
                    "secret '{scope}/{name}' value must be a string, got {}",
                    value_type(value)
                ));
            };
            if text.is_empty() {
                return Err(format!("secret '{scope}/{name}' value must not be empty"));
            }
            Ok(text.clone().into_bytes())
        }
        SettingKind::Config => {
            let schema = schema
                .cloned()
                .or_else(|| stored_config_schema(store, scope, name))
                .ok_or_else(|| {
                    format!(
                        "config '{scope}/{name}' requires a declared schema; \
                         provide `schema` on first write"
                    )
                })?;
            let ty = RuninatorType::from_json_schema_checked(&schema)
                .map_err(|err| format!("invalid config schema for '{scope}/{name}': {err}"))?;
            ty.validate_value(value).map_err(|violation| {
                format!("config '{scope}/{name}' value does not match schema: {violation}")
            })?;
            serde_json::to_vec(&StoredConfig {
                value: value.clone(),
                schema,
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
