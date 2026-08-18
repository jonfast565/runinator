//! covers the json-rpc envelope and the tool-result shapes the protocol module builds.

use super::*;

#[test]
fn success_carries_the_request_id() {
    let response = success(Value::from(7), json!({ "ok": true }));
    assert_eq!(response.get("jsonrpc").and_then(Value::as_str), Some("2.0"));
    assert_eq!(response.get("id").and_then(Value::as_i64), Some(7));
    assert_eq!(
        response.pointer("/result/ok").and_then(Value::as_bool),
        Some(true)
    );
    assert!(response.get("error").is_none());
}

#[test]
fn failure_reports_a_code_and_no_result() {
    let response = failure(Value::Null, PARSE_ERROR, "not json");
    assert!(response.get("id").is_some_and(Value::is_null));
    assert_eq!(
        response.pointer("/error/code").and_then(Value::as_i64),
        Some(PARSE_ERROR)
    );
    assert_eq!(
        response.pointer("/error/message").and_then(Value::as_str),
        Some("not json")
    );
    assert!(response.get("result").is_none());
}

#[test]
fn internal_error_uses_the_generic_code() {
    let response = internal_error(Value::from(1), "broke");
    assert_eq!(
        response.pointer("/error/code").and_then(Value::as_i64),
        Some(INTERNAL_ERROR)
    );
}

// a tool failure is an ordinary result with `isError`, not a json-rpc error: the model is meant to
// read it and act on it.
#[test]
fn a_failed_tool_call_is_a_result_not_a_transport_error() {
    let result = text_result("it did not compile", true);
    assert_eq!(result.get("isError").and_then(Value::as_bool), Some(true));
    assert_eq!(
        result.pointer("/content/0/text").and_then(Value::as_str),
        Some("it did not compile")
    );
    assert_eq!(
        result.pointer("/content/0/type").and_then(Value::as_str),
        Some("text")
    );
}

#[test]
fn json_output_is_returned_structured_as_well_as_text() {
    let result = output_result("{\"name\": \"apply\"}\n", false);
    assert_eq!(
        result
            .pointer("/structuredContent/name")
            .and_then(Value::as_str),
        Some("apply")
    );
    // the text is kept verbatim, so nothing the command printed is lost to the parse.
    assert_eq!(
        result.pointer("/content/0/text").and_then(Value::as_str),
        Some("{\"name\": \"apply\"}\n")
    );
}

#[test]
fn json_arrays_are_structured_too() {
    let result = output_result("[1, 2]", false);
    assert!(result.get("structuredContent").is_some_and(Value::is_array));
}

// a table is what most commands print without `--json`, and it is not a failure.
#[test]
fn a_table_is_returned_as_plain_text() {
    let result = output_result("name    status\napply   ok", false);
    assert!(result.get("structuredContent").is_none());
    assert_eq!(result.get("isError").and_then(Value::as_bool), Some(false));
}

// a bare scalar parses as json but says nothing more as a payload than it does as text.
#[test]
fn a_scalar_is_not_treated_as_a_payload() {
    assert!(
        output_result("42", false)
            .get("structuredContent")
            .is_none()
    );
}

#[test]
fn required_str_rejects_missing_and_blank() {
    let arguments = json!({ "command": "runs list", "blank": "   " });
    assert_eq!(
        required_str(&arguments, "command").as_deref(),
        Ok("runs list")
    );
    assert!(required_str(&arguments, "blank").is_err());
    assert!(required_str(&arguments, "absent").is_err());
}

#[test]
fn optional_str_treats_blank_as_absent() {
    let arguments = json!({ "topic": " ", "command": "runs" });
    assert_eq!(optional_str(&arguments, "topic"), None);
    assert_eq!(optional_str(&arguments, "command").as_deref(), Some("runs"));
}

#[test]
fn object_schema_closes_the_object() {
    let schema = object_schema(&[("command", json!({ "type": "string" }))], &["command"]);
    assert_eq!(schema.get("type").and_then(Value::as_str), Some("object"));
    assert_eq!(
        schema
            .pointer("/properties/command/type")
            .and_then(Value::as_str),
        Some("string")
    );
    assert_eq!(
        schema.get("required").and_then(Value::as_array),
        Some(&vec![Value::from("command")])
    );
    assert_eq!(
        schema.get("additionalProperties").and_then(Value::as_bool),
        Some(false)
    );
}
