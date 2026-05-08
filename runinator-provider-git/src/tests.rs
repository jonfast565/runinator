use super::*;

#[test]
fn test_git_provider_unsupported_action() {
    let provider = GitProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(1),
        task_id: Some(1),
        action_name: "git".into(),
        action_function: "invalid".into(),
        action_configuration: "".into(),
        parameters: json!({}),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
    };

    let result = provider.execute_service(request, None);
    assert!(result.is_err());
}
