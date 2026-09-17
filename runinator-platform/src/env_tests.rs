use super::*;

#[test]
fn parses_trimmed_values_and_positive_numbers() {
    assert_eq!(parse_value::<u64>(" 42 "), Some(42));
    assert_eq!(parse_positive_value::<u64>(" 42 "), Some(42));
    assert_eq!(parse_bool_value(" true "), Some(true));
    assert_eq!(parse_bool_value("0"), Some(false));
    assert_eq!(parse_bool_value("maybe"), None);
}

#[test]
fn positive_parser_rejects_zero_and_negative_values() {
    assert_eq!(parse_positive_value::<u64>("0"), None);
    assert_eq!(parse_positive_value::<i64>("-1"), None);
}
