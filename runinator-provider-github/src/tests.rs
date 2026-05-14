use super::*;

#[test]
fn test_github_provider_missing_token() {
    let provider = GitHubProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(1),
        action_name: "github".into(),
        action_function: "create_pr".into(),
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
