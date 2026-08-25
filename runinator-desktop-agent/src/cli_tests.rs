//! command-line override precedence and normalization.

use super::*;
use clap::Parser;

#[test]
fn execution_modes_are_mutually_exclusive() {
    for modes in [
        ["--headless", "--tui"],
        ["--headless", "--gui"],
        ["--tui", "--gui"],
    ] {
        assert!(CliArgs::try_parse_from(["desktop-agent", modes[0], modes[1]]).is_err());
    }
}

#[test]
fn gui_is_the_default_mode_and_can_be_selected_explicitly() {
    let default_args = CliArgs::try_parse_from(["desktop-agent"]).unwrap();
    assert!(!default_args.headless);
    assert!(!default_args.tui);
    assert!(!default_args.gui);

    let explicit_gui = CliArgs::try_parse_from(["desktop-agent", "--gui"]).unwrap();
    assert!(explicit_gui.gui);
}

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
