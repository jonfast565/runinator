use runinator_models::json;
use runinator_models::value::Value;

pub(crate) fn required_i64(arguments: &Value, name: &str) -> Result<i64, String> {
    arguments
        .get(name)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("missing integer argument '{name}'"))
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
