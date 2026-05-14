use super::*;

#[test]
fn test_git_provider_unsupported_action() {
    let provider = GitProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(1),
        action_name: "git".into(),
        action_function: "invalid".into(),
        parameters: json!({}),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
    };

    let result = provider.execute_service(request, None);
    assert!(result.is_err());
}

#[test]
fn metadata_includes_push_action() {
    let provider = GitProvider;
    let metadata = provider.metadata();

    let push = metadata
        .actions
        .iter()
        .find(|action| action.function_name == "push")
        .expect("push action is advertised");

    assert!(
        push.parameters
            .iter()
            .any(|parameter| parameter.name == "branch" && parameter.required)
    );
}

#[test]
fn push_requires_branch_before_execution() {
    let provider = GitProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(1),
        action_name: "git".into(),
        action_function: "push".into(),
        parameters: json!({
            "workspace": "."
        }),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
    };

    let result = provider.execute_service(request, None);
    assert!(result.is_err());
}
