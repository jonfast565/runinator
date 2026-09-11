//! injectable Claude execution keeps CLI construction and error mapping in the provider.
use super::*;
use std::sync::Mutex;

use runinator_provider_support::process_runner::ProcessResult;

#[derive(Default)]
struct MemorySink {
    events: Mutex<Vec<ProviderExecutionEvent>>,
}

impl ProviderEventSink for MemorySink {
    fn emit(&self, event: ProviderExecutionEvent) {
        self.events.lock().unwrap().push(event);
    }
}

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

#[test]
fn mission_mcp_uses_the_fixed_capability_reduced_server() {
    let params: ClaudeCodeParams = serde_json::from_value(
        json!({
            "prompt": "initial task",
            "mission_mcp": true,
            "mission_id": "018f5f7c-4b74-7f44-8fd1-cde6b5c4d111",
        })
        .into(),
    )
    .unwrap();
    let argv = build_claude_harness_argv(&params);
    let config = argv
        .windows(2)
        .find(|values| values[0] == "--mcp-config")
        .map(|values| &values[1])
        .expect("mission MCP config");
    let config: Value = serde_json::from_str(config).unwrap();
    assert_eq!(
        config
            .pointer("/mcpServers/runinator-mission/command")
            .and_then(Value::as_str),
        Some("runinatorctl")
    );
    assert_eq!(
        config
            .pointer("/mcpServers/runinator-mission/args/3")
            .and_then(Value::as_str),
        Some("018f5f7c-4b74-7f44-8fd1-cde6b5c4d111")
    );
    assert!(argv.contains(&"--strict-mcp-config".to_string()));
    assert!(argv.contains(&"--restricted".to_string()));
    assert!(
        argv.windows(2)
            .any(|values| values == ["--permission-prompts", "none"])
    );
}

#[test]
fn mission_mcp_rejects_a_caller_authored_server_config() {
    let request = ProviderExecutionRequest {
        run_id: None,
        action_name: "ai-command".into(),
        action_function: "claude_code".into(),
        parameters: json!({
            "binary": "test-claude",
            "prompt": "prompt text",
            "mission_mcp": true,
            "mcp_config": "{\"mcpServers\":{\"attacker\":{\"command\":\"sh\"}}}",
        }),
        timeout_secs: 5,
        artifact_dir: String::new(),
        events_jsonl_path: String::new(),
        idempotency_key: None,
        workspace_path: None,
        execution_profile: None,
    };
    let error = run_claude_code(&request, None, CancellationToken::new(), &Runner).unwrap_err();
    assert!(error.to_string().contains(CLAUDE_INPUT.code));
}

#[test]
fn parsed_stdout_is_emitted_once_as_progress() {
    let sink = Arc::new(MemorySink::default());
    let event_sink: Arc<dyn ProviderEventSink> = sink.clone();
    let mut result = None;
    let mut budget = HarnessEventBudget::default();
    handle_harness_line(
        &HarnessLine {
            stream: "stdout".into(),
            content: "{\"type\":\"assistant\",\"message\":\"hello\"}".into(),
            truncated: false,
        },
        Some(&event_sink),
        &mut result,
        &mut budget,
    );
    assert_eq!(sink.events.lock().unwrap().len(), 1);
    assert!(matches!(
        sink.events.lock().unwrap().first(),
        Some(ProviderExecutionEvent::Progress { kind, .. }) if kind == "claude.assistant"
    ));
}

#[test]
fn accepted_steering_is_retained_even_after_output_budget_is_exhausted() {
    let sink = Arc::new(MemorySink::default());
    let event_sink: Arc<dyn ProviderEventSink> = sink.clone();
    let (sender, receiver) = mpsc::channel();
    sender
        .send(ProviderTerminalControl::Input {
            data: "inspect the retry path".into(),
        })
        .unwrap();
    let mut terminal = Some(receiver);
    let mut stdin = Some(Vec::new());
    let mut budget = HarnessEventBudget {
        events: MAX_HARNESS_EVENTS,
        bytes: MAX_HARNESS_EVENT_BYTES,
    };

    drain_harness_controls(&mut terminal, &mut stdin, Some(&event_sink), &mut budget).unwrap();

    let encoded = String::from_utf8(stdin.unwrap()).unwrap();
    assert!(encoded.contains("inspect the retry path"));
    assert!(matches!(
        sink.events.lock().unwrap().first(),
        Some(ProviderExecutionEvent::Progress { kind, payload })
            if kind == "claude.steering"
                && payload.get("message").and_then(Value::as_str)
                    == Some("inspect the retry path")
    ));
}

#[test]
fn harness_reader_truncates_a_single_line_without_losing_the_next_line() {
    let mut reader = BufReader::new("abcdef\nok\n".as_bytes());
    assert_eq!(
        read_bounded_line(&mut reader, 3).unwrap(),
        Some(("abc…[truncated]".into(), true))
    );
    assert_eq!(
        read_bounded_line(&mut reader, 3).unwrap(),
        Some(("ok".into(), false))
    );
    assert_eq!(read_bounded_line(&mut reader, 3).unwrap(), None);
}
