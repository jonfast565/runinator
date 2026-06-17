use super::*;
use runinator_models::json;
use runinator_models::runs::ProviderExecutionRequest;

#[test]
fn metadata_includes_comments_action() {
    let provider = JiraProvider;
    let metadata = provider.metadata();
    let comments = metadata
        .actions
        .iter()
        .find(|action| action.function_name == "comments")
        .expect("comments action is advertised");
    assert!(
        comments
            .results
            .iter()
            .any(|result| result.name == "images")
    );
    assert!(
        comments
            .parameters
            .iter()
            .any(|p| p.name == "download_dir" && !p.required)
    );
}

#[test]
fn renders_adf_comment_body_with_image() {
    let body = serde_json::json!({
        "type": "doc",
        "version": 1,
        "content": [
            {
                "type": "paragraph",
                "content": [
                    { "type": "text", "text": "Please fix the layout, see " },
                    { "type": "text", "text": "this screenshot:" }
                ]
            },
            {
                "type": "mediaSingle",
                "content": [
                    { "type": "media", "attrs": { "type": "file", "id": "abc-123", "alt": "broken-button.png" } }
                ]
            }
        ]
    });
    let rendered = crate::comments::render_comment_body(Some(&body));
    assert!(rendered.contains("Please fix the layout, see this screenshot:"));
    assert!(rendered.contains("[image: broken-button.png]"));
}

#[test]
fn renders_legacy_string_comment_body() {
    let body = serde_json::json!("  just a plain text comment  ");
    let rendered = crate::comments::render_comment_body(Some(&body));
    assert_eq!(rendered, "just a plain text comment");
}

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
