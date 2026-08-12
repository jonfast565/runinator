//! agent command argument parsing.

use super::*;

#[test]
fn enrollment_ttl_accepts_documented_units() {
    assert_eq!(parse_ttl("30s").unwrap(), 30);
    assert_eq!(parse_ttl("15m").unwrap(), 900);
    assert_eq!(parse_ttl("2h").unwrap(), 7200);
    assert_eq!(parse_ttl("1d").unwrap(), 86400);
    assert!(parse_ttl("0m").is_err());
    assert!(parse_ttl("15 minutes").is_err());
}

#[test]
fn enrollment_labels_require_key_value_pairs() {
    let labels = parse_labels(&["site=home".to_string(), "gpu=true".to_string()]).unwrap();
    assert_eq!(labels.get("site").map(String::as_str), Some("home"));
    assert!(parse_labels(&["site".to_string()]).is_err());
}
