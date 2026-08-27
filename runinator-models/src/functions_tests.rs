//! covers the function domain's derived views: catalog entry -> action metadata and binding, digest
//! validation, and the action deserializer's handling of a binding.

use super::*;
use crate::providers::{ParameterMetadata, ResultMetadata};
use crate::types::RuninatorType;

fn entry() -> FunctionCatalogEntry {
    FunctionCatalogEntry {
        package_id: Uuid::from_u128(1),
        package_name: "image_tools".into(),
        namespace: None,
        version_id: Uuid::from_u128(2),
        version: 3,
        export_id: Uuid::from_u128(3),
        export_name: "resize".into(),
        artifact_digest: format!("sha256:{}", "a".repeat(64)),
        description: Some("resize an image".into()),
        input: vec![
            ParameterMetadata::required("source", RuninatorType::String),
            ParameterMetadata::optional("width", RuninatorType::Integer),
        ],
        output: vec![ResultMetadata::new("uri", RuninatorType::String)],
        aliases: vec!["production".into()],
    }
}

#[test]
fn derives_the_authoring_provider_name() {
    assert_eq!(entry().provider_name(), "functions.image_tools");

    let mut namespaced = entry();
    namespaced.namespace = Some("media".into());
    assert_eq!(namespaced.provider_name(), "functions.media.image_tools");
}

#[test]
fn derives_action_metadata_a_compiler_can_type_against() {
    let metadata = entry().action_metadata();
    assert_eq!(metadata.function_name, "resize");
    assert_eq!(metadata.parameters.len(), 2);
    assert_eq!(metadata.results.len(), 1);
    // packaged code runs a container, so it can never be evaluated in the reducer.
    assert!(!metadata.pure);

    // the derived types are what author-time checking uses, so they must round-trip the required
    // and optional distinction the manifest declared.
    let parameters = metadata.parameters_type();
    assert!(format!("{parameters:?}").contains("source"));
}

#[test]
fn binding_carries_enough_to_decompile_without_a_catalog() {
    let binding = entry().binding();
    assert_eq!(binding.call_path(), "functions.image_tools.resize");
    assert_eq!(binding.version, 3);
    // ids pin the exact code; names make the call renderable even if the package is later deleted.
    assert_eq!(binding.export_id, Uuid::from_u128(3));
}

#[test]
fn validates_artifact_digests() {
    assert!(artifact::is_valid_digest(&format!(
        "sha256:{}",
        "a".repeat(64)
    )));
    assert!(!artifact::is_valid_digest(&"a".repeat(64)));
    assert!(!artifact::is_valid_digest("sha256:short"));
    assert!(!artifact::is_valid_digest(&format!(
        "sha256:{}",
        "z".repeat(64)
    )));
    assert_eq!(artifact::digest_from_hex("ABC"), "sha256:abc");
}

#[test]
fn resource_limits_default_to_a_bounded_sandbox() {
    let limits = FunctionResourceLimits::default();
    // an omitted limit must mean "the default", never "unlimited".
    assert!(limits.timeout_seconds > 0);
    assert!(limits.memory_mb > 0);
    assert!(limits.pids > 0);
    assert!(!limits.network, "network must be opt-in");

    // a manifest that specifies nothing still deserializes to those defaults.
    let parsed: FunctionResourceLimits = serde_json::from_str("{}").unwrap();
    assert_eq!(parsed, limits);
}

#[test]
fn a_workflow_action_round_trips_its_binding() {
    use crate::workflows::WorkflowAction;

    let action = WorkflowAction {
        provider: FUNCTIONS_PROVIDER.into(),
        function: FUNCTIONS_INVOKE.into(),
        timeout_seconds: 30,
        configuration: Default::default(),
        mcp_enabled: false,
        tags: Vec::new(),
        required_labels: Default::default(),
        workspace_affinity: None,
        idempotency_key: None,
        function_binding: Some(entry().binding()),
    };
    let encoded = serde_json::to_string(&action).unwrap();
    let decoded: WorkflowAction = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, action);

    // the binding must survive as a declared field rather than being folded into `configuration`,
    // which is what the deserializer does with any key it does not know.
    assert!(
        decoded
            .configuration
            .as_value()
            .get("function_binding")
            .is_none()
    );
    assert_eq!(
        decoded.function_binding.map(|binding| binding.call_path()),
        Some("functions.image_tools.resize".to_string())
    );
}

#[test]
fn an_ordinary_action_still_has_no_binding() {
    use crate::workflows::WorkflowAction;

    let decoded: WorkflowAction =
        serde_json::from_str(r#"{"provider":"github","function":"issue","repo":"a/b"}"#).unwrap();
    assert!(decoded.function_binding.is_none());
    // and the unknown key still folds into configuration, as it did before the field existed.
    assert!(decoded.configuration.as_value().get("repo").is_some());
}
