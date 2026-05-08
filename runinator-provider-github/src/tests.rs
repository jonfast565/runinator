use super::*;

#[test]
fn test_github_provider_missing_token() {
    let provider = GitHubProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(1),
        task_id: Some(1),
        action_name: "github".into(),
        action_function: "create_pr".into(),
        action_configuration: "".into(),
        parameters: json!({
            "owner": "test",
            "repo": "test"
        }),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
    };

    let result = provider.execute_service(request, None);
    assert!(result.is_err());
}
