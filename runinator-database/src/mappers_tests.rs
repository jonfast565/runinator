use super::*;
use serde_json::json;

#[test]
fn test_parse_json() {
    assert_eq!(parse_json("{\"a\":1}".to_string()), json!({"a":1}));
    assert_eq!(parse_json("invalid".to_string()), Value::Null);
}
