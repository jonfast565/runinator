use super::*;

#[test]
fn test_ai_command_provider_execution() {
    let provider = AiCommandProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(1),
        action_name: "ai".into(),
        action_function: "cmd".into(),
        parameters: json!({
            "command": "cat",
            "input": { "test": "data" }
        }),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
    };

    let result = provider.execute_service(request, None).unwrap();
    assert_eq!(result.message.unwrap(), "AI command completed");
    let output = result.output_json.unwrap();
    assert_eq!(output["test"], "data");
}

#[test]
fn test_ai_command_fails_on_nonzero_exit() {
    let provider = AiCommandProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(1),
        action_name: "ai".into(),
        action_function: "cmd".into(),
        parameters: json!({
            "command": "exit 1",
        }),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
    };

    let result = provider.execute_service(request, None);
    assert!(result.is_err());
}
