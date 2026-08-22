//! the JSON-RPC envelope and the shape of a tool result.
//!
//! pure: nothing here contacts the web service or reads a descriptor, which is what lets the framing
//! rules be asserted without a server on the other end of the pipe.

use runinator_models::json;
use runinator_models::value::Value;

/// the protocol revision this server implements.
pub(crate) const PROTOCOL_VERSION: &str = "2025-06-18";

/// JSON-RPC's "the server broke" code. Use it for tool failures that are not framing or transport
/// failures.
const INTERNAL_ERROR: i64 = -32603;

/// JSON-RPC parse error for a line that is not JSON.
pub(crate) const PARSE_ERROR: i64 = -32700;

/// a successful response carrying `result`.
pub(crate) fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// an error response. `id` is null for a request that could not be read far enough to have one.
pub(crate) fn failure(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message.into() }
    })
}

/// an error response using the generic internal code.
pub(crate) fn internal_error(id: Value, message: impl Into<String>) -> Value {
    failure(id, INTERNAL_ERROR, message)
}

/// A tool result carrying text.
///
/// Report a failed tool call as `isError` on an ordinary result. This lets the model see the command
/// output and failure together.
pub(crate) fn text_result(text: impl Into<String>, is_error: bool) -> Value {
    json!({
        "content": [{ "type": "text", "text": text.into() }],
        "isError": is_error,
    })
}

/// a tool result carrying text plus the structured payload it was rendered from.
pub(crate) fn structured_result(
    text: impl Into<String>,
    structured: Value,
    is_error: bool,
) -> Value {
    json!({
        "content": [{ "type": "text", "text": text.into() }],
        "structuredContent": structured,
        "isError": is_error,
    })
}

/// a tool result for output that is text but may also be json.
///
/// commands print json when asked to and a table otherwise, and the model gets more out of the
/// parsed form when there is one — but a table is not a failure, so an unparsable body is still
/// returned rather than reported as one.
pub(crate) fn output_result(text: &str, is_error: bool) -> Value {
    match serde_json::from_str::<Value>(text.trim()) {
        Ok(structured) if structured.is_object() || structured.is_array() => {
            structured_result(text, structured, is_error)
        }
        _ => text_result(text, is_error),
    }
}

/// a required string argument.
pub(crate) fn required_str(arguments: &Value, name: &str) -> Result<String, String> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("missing argument '{name}'"))
}

/// an optional string argument, treating an empty string as absent.
pub(crate) fn optional_str(arguments: &Value, name: &str) -> Option<String> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
}

/// a json-schema object with the given properties and required names.
pub(crate) fn object_schema(properties: &[(&str, Value)], required: &[&str]) -> Value {
    let mut map = runinator_models::value::Map::new();
    for (name, schema) in properties {
        map.insert((*name).into(), schema.clone());
    }
    json!({
        "type": "object",
        "properties": map,
        "required": required,
        "additionalProperties": false,
    })
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
