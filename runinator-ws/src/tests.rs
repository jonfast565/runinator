use crate::repository::*;
use serde_json::json;

#[test]
fn validates_json_schema() {
    let schema = json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "age": { "type": "integer" }
        },
        "required": ["name"]
    });

    assert!(validate_json_schema(&schema, &json!({ "name": "bob", "age": 42 })).is_ok());
    assert!(validate_json_schema(&schema, &json!({ "name": "bob" })).is_ok());
    assert!(validate_json_schema(&schema, &json!({ "age": 42 })).is_err());
    assert!(validate_json_schema(&schema, &json!({ "name": 123 })).is_err());
}

#[test]
fn merges_json_objects() {
    let defaults = json!({ "a": 1, "b": 2 });
    let parameters = json!({ "b": 3, "c": 4 });
    let merged = merge_json_object(&defaults, &parameters);
    assert_eq!(merged, json!({ "a": 1, "b": 3, "c": 4 }));
}
