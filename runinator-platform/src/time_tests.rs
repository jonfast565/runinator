use super::*;

#[test]
fn converts_seconds_and_milliseconds_to_utc() {
    assert_eq!(from_unix_seconds(1_000).unwrap().timestamp(), 1_000);
    assert_eq!(
        from_unix_millis_or_seconds(1_234_567_890_123)
            .unwrap()
            .timestamp_subsec_millis(),
        123
    );
}

#[test]
fn formats_and_parses_common_durations() {
    assert_eq!(format_duration(Duration::from_secs(125)), "2m 05s");
    assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7_200));
    assert_eq!(parse_optional_duration("off").unwrap(), None);
}
