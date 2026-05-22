use super::*;
use runinator_models::runs::ProviderExecutionRequest;
use serde_json::json;

#[test]
fn test_jira_provider_missing_base_url() {
    let provider = JiraProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(1),
        action_name: "jira".into(),
        action_function: "search".into(),
        parameters: json!({
            "token": "test"
        }),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
    };

    let result = provider.execute_service(
        request,
        None,
        runinator_plugin::cancel::CancellationToken::new(),
    );
    assert!(result.is_err());
}
