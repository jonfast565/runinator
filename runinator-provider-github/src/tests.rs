use super::*;

#[test]
fn test_github_provider_missing_token() {
    let provider = GitHubProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(uuid::Uuid::now_v7()),
        action_name: "github".into(),
        action_function: "create_pr".into(),
        parameters: runinator_models::json!({
            "owner": "test",
            "repo": "test"
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
fn metadata_includes_merge_pr_action() {
    let provider = GitHubProvider;
    let metadata = provider.metadata();

    let merge_pr = metadata
        .actions
        .iter()
        .find(|action| action.function_name == "merge_pr")
        .expect("merge_pr action is advertised");

    assert!(
        merge_pr
            .parameters
            .iter()
            .any(|parameter| parameter.name == "pull_number" && parameter.required)
    );
}

#[test]
fn metadata_includes_checks_summary_action() {
    let provider = GitHubProvider;
    let metadata = provider.metadata();

    let checks_summary = metadata
        .actions
        .iter()
        .find(|action| action.function_name == "checks_summary")
        .expect("checks_summary action is advertised");

    assert!(
        checks_summary
            .results
            .iter()
            .any(|result| result.name == "status")
    );
}

#[test]
fn metadata_includes_collaboration_actions() {
    let provider = GitHubProvider;
    let metadata = provider.metadata();

    for (function, param) in [
        ("add_comment", "body"),
        ("request_reviewers", "reviewers"),
        ("add_assignees", "assignees"),
    ] {
        let action = metadata
            .actions
            .iter()
            .find(|action| action.function_name == function)
            .unwrap_or_else(|| panic!("{function} action is advertised"));
        assert!(
            action.parameters.iter().any(|p| p.name == param),
            "{function} exposes {param}"
        );
    }
}

#[test]
fn request_reviewers_requires_a_reviewer() {
    let provider = GitHubProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(uuid::Uuid::now_v7()),
        action_name: "github".into(),
        action_function: "request_reviewers".into(),
        parameters: runinator_models::json!({
            "token": "t",
            "owner": "o",
            "repo": "r",
            "pull_number": "1"
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
fn summarizes_check_runs() {
    let passed = summarize_check_runs(json!({
        "check_runs": [
            { "status": "completed", "conclusion": "success" },
            { "status": "completed", "conclusion": "neutral" }
        ]
    }));
    assert_eq!(passed["status"], "passed");
    assert_eq!(passed["passed"], 2);

    let pending = summarize_check_runs(json!({
        "check_runs": [
            { "status": "queued", "conclusion": null },
            { "status": "completed", "conclusion": "success" }
        ]
    }));
    assert_eq!(pending["status"], "pending");
    assert_eq!(pending["pending"], 1);

    let failed = summarize_check_runs(json!({
        "check_runs": [
            { "status": "completed", "conclusion": "failure" }
        ]
    }));
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["failed"], 1);
}
