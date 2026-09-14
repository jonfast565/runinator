use super::*;

#[test]
fn cli_resolves_runtime_counts_and_disables_optional_components() {
    let cli = Cli::try_parse_from([
        "runinator-standalone",
        "--workers",
        "3",
        "--engines",
        "2",
        "--wakers",
        "0",
        "--no-desktop-agent",
        "--no-default-pack",
        "start",
        "--foreground",
    ])
    .unwrap();

    let config = cli.resolved_config().unwrap();
    assert_eq!(config.workers, 3);
    assert_eq!(config.engines, 2);
    assert_eq!(config.wakers, 0);
    assert!(!config.desktop_agent);
    assert!(config.packs.is_empty());
}
