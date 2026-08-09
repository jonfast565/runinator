use super::*;
use serde_json::json;

#[test]
fn parses_nested_envelope() {
    let value = json!({ "runinator": { "autoApply": true, "serviceUrl": "http://x/" } });
    let config = Config::from_value(Some(&value));
    assert!(config.auto_apply);
    assert_eq!(config.service_url.as_deref(), Some("http://x/"));
}

#[test]
fn parses_flat_object() {
    let value = json!({ "autoApply": true });
    let config = Config::from_value(Some(&value));
    assert!(config.auto_apply);
    assert!(config.service_url.is_none());
}

#[test]
fn empty_service_url_is_none() {
    let value = json!({ "serviceUrl": "   " });
    assert!(Config::from_value(Some(&value)).service_url.is_none());
}

#[test]
fn missing_options_default_off() {
    let config = Config::from_value(None);
    assert!(!config.auto_apply);
    assert!(config.service_url.is_none());
}
