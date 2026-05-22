use super::*;

#[test]
fn test_approval_provider_execution() {
    let provider = ApprovalProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(1),
        action_name: "approval".into(),
        action_function: "prepare".into(),
        parameters: json!({
            "approval_type": "deploy",
            "prompt": "Approve production deployment?"
        }),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
    };

    let result = provider
        .execute_service(
            request,
            None,
            runinator_plugin::cancel::CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(result.message.unwrap(), "Approval request prepared");
    let output = result.output_json.unwrap();
    assert_eq!(output["approval_type"], "deploy");
    assert_eq!(output["prompt"], "Approve production deployment?");
}
