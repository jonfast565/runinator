use super::*;

#[test]
fn parses_duration_units() {
    assert_eq!(parse_required_duration("30m").unwrap().as_secs(), 1800);
    assert_eq!(parse_required_duration("1h").unwrap().as_secs(), 3600);
    assert_eq!(parse_required_duration("2w").unwrap().as_secs(), 1_209_600);
}

#[test]
fn parses_disabled_retention() {
    assert!(parse_optional_duration("off").unwrap().is_none());
    assert!(parse_optional_duration("none").unwrap().is_none());
}
