//! capability-reduced MCP request fencing.

use super::*;

#[test]
fn mission_binding_fence_replaces_a_model_supplied_id() {
    let bound = Uuid::parse_str("018f5f7c-4b74-7f44-8fd1-cde6b5c4d111").unwrap();
    let arguments = json!({
        "id": "018f5f7c-4b74-7f44-8fd1-cde6b5c4d999",
        "intent": "pause",
    });

    let fenced = fence_mission_arguments(&arguments, Some(bound));

    assert_eq!(
        fenced.get("id").and_then(Value::as_str).map(str::to_owned),
        Some(bound.to_string())
    );
    assert_eq!(fenced.get("intent").and_then(Value::as_str), Some("pause"));
}
