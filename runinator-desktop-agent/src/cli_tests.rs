//! command-line override precedence and normalization.

use super::*;

#[test]
fn overrides_only_explicit_values() {
    let original = AgentConfig {
        service_url: "https://saved.example/".to_string(),
        sandbox_root: "/saved/root".to_string(),
        allow_write: true,
        max_concurrent_actions: 4,
        ..AgentConfig::default()
    };

    let applied = CliArgs {
        service_url: Some("https://cli.example/".to_string()),
        labels: Some("zone=home, runner=sync, ,".to_string()),
        max_concurrent_actions: Some(0),
        ..CliArgs::default()
    }
    .apply(original);

    assert_eq!(applied.service_url, "https://cli.example/");
    assert_eq!(applied.sandbox_root, "/saved/root");
    assert!(applied.allow_write);
    assert_eq!(applied.max_concurrent_actions, 1);
    assert_eq!(applied.extra_labels, ["zone=home", "runner=sync"]);
}

#[test]
fn invalid_enum_overrides_keep_saved_values() {
    let original = AgentConfig {
        broker_mode: BrokerMode::Direct,
        log_level: LogLevel::Debug,
        ..AgentConfig::default()
    };

    let applied = CliArgs {
        broker_mode: Some("not-a-mode".to_string()),
        log_level: Some("verbose-ish".to_string()),
        ..CliArgs::default()
    }
    .apply(original);

    assert_eq!(applied.broker_mode, BrokerMode::Direct);
    assert_eq!(applied.log_level, LogLevel::Debug);
}
