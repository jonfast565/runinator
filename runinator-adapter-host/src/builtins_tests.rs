//! built-in catalog and execution share a single registration source.
use super::*;
#[test]
fn catalog_uses_registry_metadata() {
    let catalog = builtin_catalog();
    assert_eq!(catalog.len(), 3);
    for (kind, adapter) in registry() {
        assert_eq!(catalog[kind].metadata.kind, adapter.metadata().kind);
        assert_eq!(catalog[kind].origin, "builtin");
    }
}
#[tokio::test]
async fn webhook_only_poll_keeps_checkpoint() {
    let request: AdapterPollRequest = serde_json::from_value(serde_json::json!({
        "configuration": {}, "secrets": {}, "checkpoint": {"cursor":"previous"}
    }))
    .unwrap();
    let response = registry()["generic_webhook"].poll(request).await;
    assert_eq!(
        response.checkpoint,
        serde_json::json!({"cursor":"previous"})
    );
    assert!(response.events.is_empty());
    assert!(response.error.is_some());
}

#[test]
fn polling_validation_is_owned_by_the_adapter_kind() {
    let request: AdapterValidationRequest = serde_json::from_value(serde_json::json!({
        "transport": "polling",
        "configuration": {"repositories": ["missing-separator"], "poll_interval_seconds": 10},
        "authentication": {"kind": "secrets", "secret_bindings": {"access_token": "00000000-0000-0000-0000-000000000001"}}
    }))
    .unwrap();
    let response = registry()["github"].validate(request);
    assert!(!response.is_valid());
    assert!(
        response
            .issues
            .iter()
            .any(|issue| issue.path == "configuration.repositories")
    );
    assert!(
        response
            .issues
            .iter()
            .any(|issue| issue.path == "configuration.poll_interval_seconds")
    );
}
