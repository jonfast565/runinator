use runinator_database::interfaces::DatabaseImpl;
use runinator_models::settings::SettingKind;
use runinator_models::types::RuninatorType;
use runinator_models::value::{Map, Value};
use runinator_utilities::secret_cipher::SecretCipher;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

// the persisted form of a config entry: the json value plus the schema it was validated against.
#[derive(Debug, Serialize, Deserialize)]
struct StoredConfig {
    value: Value,
    schema: Value,
}

fn settings_cipher() -> SecretCipher {
    SecretCipher::from_env()
}

/// the config reference tree `{ <scope>: { <name>: <value> } }`.
pub async fn config_tree<T: DatabaseImpl>(db: &T) -> Value {
    let cipher = settings_cipher();
    let Ok(entries) = db.list_settings().await else {
        return Value::Object(Map::new());
    };
    let mut root = Map::new();
    for entry in entries {
        if entry.kind != SettingKind::Config {
            continue;
        }
        let Some(plaintext) = cipher.try_decrypt(&entry.value) else {
            continue;
        };
        let value = decode_config_value(&plaintext);
        let scope = root
            .entry(entry.scope)
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(scope) = scope.as_object_mut() {
            scope.insert(entry.name, value);
        }
    }
    Value::Object(root)
}

/// the config type tree `{ <scope>: { <name>: <type> } }` used to type-check config refs.
pub async fn config_type_tree<T: DatabaseImpl>(db: &T) -> RuninatorType {
    let cipher = settings_cipher();
    let Ok(entries) = db.list_settings().await else {
        return RuninatorType::map(RuninatorType::Any);
    };
    let mut scopes: BTreeMap<String, BTreeMap<String, RuninatorType>> = BTreeMap::new();
    for entry in entries {
        if entry.kind != SettingKind::Config {
            continue;
        }
        let Some(plaintext) = cipher.try_decrypt(&entry.value) else {
            continue;
        };
        let Some(ty) = stored_config_type(&plaintext) else {
            continue;
        };
        scopes
            .entry(entry.scope)
            .or_default()
            .insert(entry.name, ty);
    }
    let scope_fields = scopes.into_iter().map(|(scope, names)| {
        (
            scope,
            RuninatorType::open_structure(names, RuninatorType::Any),
        )
    });
    RuninatorType::open_structure(scope_fields, RuninatorType::Any)
}

/// decode a stored config payload back to its json value.
pub fn decode_config_value(bytes: &[u8]) -> Value {
    if let Ok(stored) = serde_json::from_slice::<StoredConfig>(bytes) {
        return stored.value;
    }
    serde_json::from_slice::<Value>(bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(bytes).into_owned()))
}

/// the schema pinned in a stored config payload, if it carries one.
pub fn decode_config_schema(bytes: &[u8]) -> Option<Value> {
    serde_json::from_slice::<StoredConfig>(bytes)
        .ok()
        .map(|stored| stored.schema)
}

/// the pinned type of a stored config slot, decoded from its bytes.
pub fn stored_config_type(bytes: &[u8]) -> Option<RuninatorType> {
    match decode_config_schema(bytes) {
        Some(schema) => Some(RuninatorType::from_json_schema(&schema)),
        None => Some(RuninatorType::infer_from_value(&decode_config_value(bytes))),
    }
}

/// validate a value for its kind and produce the bytes to persist.
pub fn validate_and_encode(
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
