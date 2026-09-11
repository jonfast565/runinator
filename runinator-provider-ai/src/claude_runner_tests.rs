//! injectable Claude execution keeps CLI construction and error mapping in the provider.
use super::*;
use runinator_provider_support::process_runner::ProcessResult;
struct Runner;
impl ProcessRunner for Runner {
    fn run(&self, request: ProcessRequest<'_>) -> Result<ProcessResult, ProcessFailure> {
        assert_eq!(request.command.get_program(), "test-claude");
        let args: Vec<_> = request.command.get_args().collect();
        assert!(args.contains(&std::ffi::OsStr::new("prompt text")));
        assert_eq!(request.timeout, Duration::from_secs(5));
        Err(ProcessFailure::TimedOut)
    }
}
#[test]
fn preserves_timeout_descriptor() {
    let request = ProviderExecutionRequest {
        run_id: None,
        action_name: "ai-command".into(),
        action_function: "claude_code".into(),
        parameters: json!({"binary":"test-claude","prompt":"prompt text"}),
        timeout_secs: 5,
        artifact_dir: String::new(),
        events_jsonl_path: String::new(),
        idempotency_key: None,
        workspace_path: None,
        execution_profile: None,
    };
    let error = run_claude_code(&request, None, CancellationToken::new(), &Runner).unwrap_err();
    assert!(error.to_string().contains(CLAUDE_TIMEOUT.code));
}

#[test]
fn harness_uses_stream_json_and_keeps_steering_as_a_user_message() {
    let params: ClaudeCodeParams = serde_json::from_value(
        json!({
            "prompt": "initial task",
            "resume_session": "session-123",
            "mcp_config": "/tmp/mission-mcp.json",
            "max_turns": 7,
            "permission_mode": "acceptEdits",
        })
        .into(),
    )
    .unwrap();
    let argv = build_claude_harness_argv(&params);
    assert!(
        argv.windows(2)
            .any(|values| values == ["--input-format", "stream-json"])
    );
    assert!(
        argv.windows(2)
            .any(|values| values == ["--output-format", "stream-json"])
    );
    assert!(
        argv.windows(2)
            .any(|values| values == ["--resume", "session-123"])
    );
    assert!(
        argv.windows(2)
            .any(|values| values == ["--mcp-config", "/tmp/mission-mcp.json"])
    );
    assert!(argv.windows(2).any(|values| values == ["--max-turns", "7"]));
    assert!(!argv.contains(&"initial task".to_string()));

    let mut bytes = Vec::new();
    write_harness_message(&mut bytes, "refocus on the failing test").unwrap();
    let encoded = String::from_utf8(bytes).unwrap();
    let value: Value = serde_json::from_str(encoded.trim()).unwrap();
    assert_eq!(value.pointer("/type").and_then(Value::as_str), Some("user"));
    assert_eq!(
        value
            .pointer("/message/content/0/text")
            .and_then(Value::as_str),
        Some("refocus on the failing test")
    );
}
