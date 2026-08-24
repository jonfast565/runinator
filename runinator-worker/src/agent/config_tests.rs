//! covers broker selection: relay derivation, direct passthrough, and mode parsing.

use super::*;

fn selection(mode: BrokerMode) -> BrokerSelection {
    BrokerSelection {
        mode,
        service_url: "https://runinator.example.com/".to_string(),
        direct_backend: "tcp".to_string(),
        direct_endpoint: "10.0.0.4:7070".to_string(),
        effect_topic: "runinator.effects".to_string(),
        infrastructure_effect_topic: "runinator.effects.infrastructure".to_string(),
        control_topic: "runinator.control".to_string(),
        agent_topic: "runinator.agent".to_string(),
        effect_result_topic: "runinator.effect-results".to_string(),
        ingress_topic: "runinator.ingress".to_string(),
        client_id: "test-agent".to_string(),
        api_key: Some("secret".to_string()),
    }
}

#[test]
fn relay_mode_targets_the_web_service_and_ignores_the_direct_endpoint() {
    let (config, description) = selection(BrokerMode::Relay).resolve().unwrap();
    assert_eq!(config.broker_backend, "ws");
    assert_eq!(
        config.broker_endpoint,
        "wss://runinator.example.com/ws/broker"
    );
    assert_eq!(config.api_key.as_deref(), Some("secret"));
    assert!(description.starts_with("relay via wss://"), "{description}");
}

#[test]
fn direct_mode_passes_the_backend_through_untouched() {
    let (config, description) = selection(BrokerMode::Direct).resolve().unwrap();
    assert_eq!(config.broker_backend, "tcp");
    assert_eq!(config.broker_endpoint, "10.0.0.4:7070");
    assert_eq!(description, "direct tcp @ 10.0.0.4:7070");
}

// A direct-mode agent must not be blocked by an unusable service URL. It never derives a relay URL,
// so the URL only has to be good enough for the API client.
#[test]
fn direct_mode_does_not_validate_the_service_url() {
    let mut selection = selection(BrokerMode::Direct);
    selection.service_url = "ftp://nope".to_string();
    assert!(selection.resolve().is_ok());
}

#[test]
fn relay_mode_reports_an_unusable_service_url() {
    let mut selection = selection(BrokerMode::Relay);
    selection.service_url = "ftp://nope".to_string();
    assert!(selection.resolve().is_err());
}

#[test]
fn broker_mode_parses_its_own_spelling() {
    for mode in [BrokerMode::Relay, BrokerMode::Direct] {
        assert_eq!(BrokerMode::parse(mode.as_str()), Some(mode));
    }
    assert_eq!(BrokerMode::parse("  RELAY "), Some(BrokerMode::Relay));
    assert_eq!(BrokerMode::parse("sideways"), None);
}
