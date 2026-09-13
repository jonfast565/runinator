//! Worker-side materialization of declarative provider credential injections.

use super::*;
use runinator_models::providers::{CredentialInjection, ParameterMetadata, RuninatorType};

#[test]
fn credential_materialization_targets_parameters_environment_arguments_and_headers() {
    let action = ActionMetadata::new("run", "run").with_parameters(vec![
        ParameterMetadata::optional("token", RuninatorType::String)
            .secret()
            .inject(CredentialInjection::Parameter {
                name: "api_token".into(),
                template: "${secret}".into(),
            })
            .inject(CredentialInjection::Environment {
                name: "API_TOKEN".into(),
                template: "key-${secret}".into(),
            })
            .inject(CredentialInjection::Arguments {
                values: vec!["--token".into(), "${secret}".into()],
            })
            .inject(CredentialInjection::Header {
                name: "Authorization".into(),
                template: "Bearer ${secret}".into(),
            }),
        ParameterMetadata::optional("api_token", RuninatorType::String),
    ]);

    let (parameters, injections) =
        materialize_credential_injections(&action, runinator_models::json!({"token":"s3cr3t"}))
            .unwrap();

    assert_eq!(parameters.get("token"), None);
    assert_eq!(
        parameters.get("api_token").and_then(Value::as_str),
        Some("s3cr3t")
    );
    assert_eq!(
        injections.environment.get("API_TOKEN").map(String::as_str),
        Some("key-s3cr3t")
    );
    assert_eq!(injections.arguments, ["--token", "s3cr3t"]);
    assert_eq!(
        injections.headers.get("Authorization").map(String::as_str),
        Some("Bearer s3cr3t")
    );
}
