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

#[test]
fn legacy_flags_seed_the_policy_until_server_settings_are_saved() {
    let cli = Cli::try_parse_from([
        "runinator-archiver",
        "--database",
        "sqlite",
        "--database-url",
        "sqlite::memory:",
        "--interval",
        "2h",
        "--batch-size",
        "250",
        "--workflow-run-retention",
        "off",
        "--dry-run",
    ])
    .unwrap();
    let policy = Config::from_cli(cli).unwrap().bootstrap_archiver_settings();

    assert_eq!(policy.interval_seconds, 7_200);
    assert_eq!(policy.batch_size, 250);
    assert_eq!(policy.workflow_run_retention_seconds, 0);
    assert!(policy.dry_run);
}
