//! credential injection helpers apply only worker-materialized values.

use super::*;
use runinator_models::runs::MaterializedCredentialInjections;
use std::collections::BTreeMap;

fn request() -> ProviderExecutionRequest {
    ProviderExecutionRequest {
        run_id: None,
        action_name: "test".into(),
        action_function: "run".into(),
        parameters: runinator_models::json!({}),
        timeout_secs: 30,
        artifact_dir: String::new(),
        events_jsonl_path: String::new(),
        idempotency_key: None,
        workspace_path: None,
        execution_profile: None,
        credential_injections: MaterializedCredentialInjections {
            environment: BTreeMap::from([("API_TOKEN".into(), "env-secret".into())]),
            arguments: vec!["--token".into(), "arg-secret".into()],
            headers: BTreeMap::from([("authorization".into(), "Bearer header-secret".into())]),
        },
    }
}

#[test]
fn command_credentials_are_applied_in_declared_order() {
    let mut command = Command::new("provider");
    apply_command_credentials(&mut command, &request());

    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        [
            std::ffi::OsStr::new("--token"),
            std::ffi::OsStr::new("arg-secret")
        ]
    );
    assert_eq!(
        command
            .get_envs()
            .find(|(name, _)| *name == std::ffi::OsStr::new("API_TOKEN"))
            .and_then(|(_, value)| value),
        Some(std::ffi::OsStr::new("env-secret"))
    );
}

#[test]
fn http_credentials_are_applied_as_headers() {
    let request = apply_blocking_http_credentials(
        reqwest::blocking::Client::new().get("https://example.invalid"),
        &request(),
    )
    .expect("credential headers are valid")
    .build()
    .expect("request builds");

    assert_eq!(request.headers()["authorization"], "Bearer header-secret");
}
