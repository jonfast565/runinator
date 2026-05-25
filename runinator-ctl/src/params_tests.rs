use serde_json::json;

use super::load_object;

#[test]
fn parses_key_value_params_into_json_values() {
    let params = vec![
        "name=demo".to_string(),
        "count=3".to_string(),
        "enabled=true".to_string(),
    ];

    let value = load_object(None, &params).unwrap();

    assert_eq!(
        value,
        json!({
            "name": "demo",
            "count": 3,
            "enabled": true
        })
    );
}
