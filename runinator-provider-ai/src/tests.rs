use super::*;

#[test]
fn test_ai_command_provider_execution() {
    let provider = AiCommandProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(1),
        action_name: "ai".into(),
        action_function: "execute".into(),
        parameters: json!({
            "command": "cat",
            "input": { "test": "data" }
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
        action_function: "execute".into(),
        parameters: json!({
            "command": "exit 1",
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
fn test_claude_code_stub_binary_passes_argv() {
    let provider = AiCommandProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(1),
        action_name: "ai-command".into(),
        action_function: "claude_code".into(),
        parameters: json!({
            "binary": "/bin/echo",
            "model": "claude-sonnet-4-6",
            "output_format": "text",
            "allowed_tools": "Bash Edit Read",
            "permission_mode": "acceptEdits",
            "extra_args": ["--add-dir", "/tmp"],
            "prompt": "hello-world-prompt"
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
    assert_eq!(result.message.as_deref(), Some("Claude Code completed"));
    let text = result.output_json.unwrap()["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        text.contains("--model"),
        "argv must contain --model: {text}"
    );
    assert!(
        text.contains("claude-sonnet-4-6"),
        "argv must contain model name: {text}"
    );
    assert!(
        text.contains("--output-format"),
        "argv must contain --output-format: {text}"
    );
    assert!(
        text.contains("--allowedTools"),
        "argv must contain --allowedTools: {text}"
    );
    assert!(
        text.contains("Bash Edit Read"),
        "argv must contain tool list: {text}"
    );
    assert!(
        text.contains("--permission-mode"),
        "argv must contain --permission-mode: {text}"
    );
    assert!(
        text.contains("acceptEdits"),
        "argv must contain permission mode: {text}"
    );
    assert!(
        text.contains("--add-dir"),
        "argv must contain extra args: {text}"
    );
    assert!(
        text.contains("hello-world-prompt"),
        "argv must contain prompt: {text}"
    );
}

#[test]
fn test_claude_code_nonzero_exit() {
    let provider = AiCommandProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(1),
        action_name: "ai-command".into(),
        action_function: "claude_code".into(),
        parameters: json!({
            "binary": "/bin/false",
            "prompt": "anything",
            "output_format": "text"
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
fn test_claude_code_invalid_params_missing_prompt() {
    let provider = AiCommandProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(1),
        action_name: "ai-command".into(),
        action_function: "claude_code".into(),
        parameters: json!({
            "binary": "/bin/echo"
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
        .err()
        .unwrap();
    assert!(
        err.to_string().contains("prompt"),
        "error should mention missing prompt: {err}"
    );
}

#[test]
fn test_claude_code_json_output_parsed() {
    let provider = AiCommandProvider;
    // /bin/echo prints args plus a newline; we feed valid json as the trailing positional "prompt" and the provider parses it when output_format=json.
    let request = ProviderExecutionRequest {
        run_id: Some(1),
        action_name: "ai-command".into(),
        action_function: "claude_code".into(),
        parameters: json!({
            "binary": "/bin/echo",
            "model": "claude-sonnet-4-6",
            "output_format": "text",
            "prompt": "{\"ok\":true}"
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
    let text = result.output_json.unwrap()["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("{\"ok\":true}"));
}
