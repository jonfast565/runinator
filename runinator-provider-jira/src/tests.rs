use super::*;
use runinator_models::json;
use runinator_models::runs::ProviderExecutionRequest;

#[test]
fn test_jira_provider_missing_base_url() {
    let provider = JiraProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(uuid::Uuid::now_v7()),
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

#[test]
fn test_jira_search_placeholder_base_url_is_clear() {
    // a bad config value should produce a descriptive error, not "builder error".
    let provider = JiraProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(uuid::Uuid::now_v7()),
        action_name: "jira".into(),
        action_function: "search".into(),
        parameters: json!({
            "base_url": "<<insert here>>",
            "token": "test",
            "jql": "project = ABC",
        }),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
    };

    let err = provider
        .execute_service(
            request,
            None,
            runinator_plugin::cancel::CancellationToken::new(),
        )
        .expect_err("placeholder base_url should fail");
    let message = err.to_string();
    assert!(message.contains("jira.config"), "got: {message}");
    assert!(message.contains("<<insert here>>"), "got: {message}");
    assert!(!message.contains("builder error"), "got: {message}");
}

#[test]
fn test_jira_search_empty_base_url_is_clear() {
    let provider = JiraProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(uuid::Uuid::now_v7()),
        action_name: "jira".into(),
        action_function: "search".into(),
        parameters: json!({
            "base_url": "",
            "token": "test",
            "jql": "project = ABC",
        }),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
    };

    let err = provider
        .execute_service(
            request,
            None,
            runinator_plugin::cancel::CancellationToken::new(),
        )
        .expect_err("empty base_url should fail");
    let message = err.to_string();
    assert!(message.contains("jira base_url is empty"), "got: {message}");
}
