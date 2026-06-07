use runinator_models::json;
use runinator_models::value::Value;
use uuid::Uuid;

/// extract a required uuid argument (accepts a hyphenated uuid string).
pub(crate) fn required_uuid(arguments: &Value, name: &str) -> Result<Uuid, String> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse().ok())
        .ok_or_else(|| format!("missing uuid argument '{name}'"))
}

pub(crate) fn required_value(arguments: &Value, name: &str) -> Result<Value, String> {
    arguments
        .get(name)
        .cloned()
        .ok_or_else(|| format!("missing argument '{name}'"))
}

pub(crate) fn json_tool_response(
    message: &str,
    value: Value,
    is_error: bool,
) -> Result<Value, String> {
    Ok(json!({
        "content": [{
            "type": "text",
            "text": message,
        }],
        "structuredContent": value,
        "isError": is_error,
    }))
}

pub(crate) fn json_export_response(bundle: Value) -> Result<Value, String> {
    let text = serde_json::to_string_pretty(&bundle).map_err(|err| err.to_string())?;
    Ok(json!({
        "content": [{
            "type": "text",
            "text": text,
        }],
        "structuredContent": bundle,
        "isError": false,
    }))
}
