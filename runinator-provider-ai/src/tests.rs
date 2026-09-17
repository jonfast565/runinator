use super::*;

#[derive(Default)]
struct CapturedEvents(std::sync::Mutex<Vec<runinator_models::runs::ProviderExecutionEvent>>);

impl runinator_plugin::provider::ProviderEventSink for CapturedEvents {
    fn emit(&self, event: runinator_models::runs::ProviderExecutionEvent) {
        self.0.lock().unwrap().push(event);
    }
}

#[test]
fn claude_code_defaults_to_opus_five() {
    assert_eq!(crate::params::default_model(), "claude-opus-5");
}

#[test]
fn claude_code_advertises_agent_authoring_semantics() {
    let metadata = AiCommandProvider.metadata();
    let action = metadata
        .actions
        .iter()
        .find(|action| action.function_name == "claude_code")
        .unwrap();
    let agent = action.agent.as_ref().unwrap();
    assert_eq!(agent.prompt_parameter, "prompt");
    assert_eq!(agent.response_text_pointer, "/response/result");
}

#[test]
fn codex_advertises_isolated_authentication_and_agent_semantics() {
    let metadata = AiCommandProvider.metadata();
    let action = metadata
        .actions
        .iter()
        .find(|action| action.function_name == "codex")
        .unwrap();
    assert_eq!(
        action
            .credential_scopes
            .as_ref()
            .map(|scopes| scopes.iter().map(String::as_str).collect::<Vec<_>>()),
        Some(vec!["codex"])
    );
    assert_eq!(
        action.agent.as_ref().unwrap().response_text_pointer,
        "/response/result"
    );
    assert!(action.authentication.as_ref().unwrap().required);
    assert!(action.authentication.as_ref().unwrap().allow_multiple);
    let api_key = action
        .parameters
        .iter()
        .find(|parameter| parameter.name == "api_key")
        .unwrap();
    assert!(api_key.secret);
    assert_eq!(api_key.credential_injections.len(), 1);
}

#[test]
fn test_ai_command_provider_execution() {
    let provider = AiCommandProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(uuid::Uuid::now_v7()),
        action_name: "ai".into(),
        action_function: "execute".into(),
        parameters: json!({
            "command": "cat",
            "input": { "test": "data" }
        }),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
        idempotency_key: None,
        workspace_path: None,
        execution_profile: None,
        credential_injections: Default::default(),
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
        run_id: Some(uuid::Uuid::now_v7()),
        action_name: "ai".into(),
        action_function: "execute".into(),
        parameters: json!({
            "command": "exit 1",
        }),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
        idempotency_key: None,
        workspace_path: None,
        execution_profile: None,
        credential_injections: Default::default(),
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
        run_id: Some(uuid::Uuid::now_v7()),
        action_name: "ai-command".into(),
        action_function: "claude_code".into(),
        parameters: json!({
            "binary": "/bin/echo",
            "model": "claude-opus-5",
            "output_format": "text",
            "allowed_tools": "Bash Edit Read",
            "permission_mode": "acceptEdits",
            "extra_args": ["--add-dir", "/tmp"],
            "prompt": "hello-world-prompt"
        }),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
        idempotency_key: None,
        workspace_path: None,
        execution_profile: None,
        credential_injections: Default::default(),
    };

    let result = provider
        .execute_service(
            request,
            None,
            runinator_plugin::cancel::CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(result.message.as_deref(), Some("Claude Code completed"));
    let text = result.output_json.unwrap()["response"]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        text.contains("--model"),
        "argv must contain --model: {text}"
    );
    assert!(
        text.contains("claude-opus-5"),
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
        run_id: Some(uuid::Uuid::now_v7()),
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
        idempotency_key: None,
        workspace_path: None,
        execution_profile: None,
        credential_injections: Default::default(),
    };

    let result = provider.execute_service(
        request,
        None,
        runinator_plugin::cancel::CancellationToken::new(),
    );
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn failed_claude_attempt_still_emits_terminal_usage() {
    use std::{io::Write, os::unix::fs::PermissionsExt, sync::Arc};

    let mut binary = tempfile::Builder::new().suffix(".sh").tempfile().unwrap();
    writeln!(
        binary,
        "#!/bin/sh\nprintf '%s\\n' '{{\"usage\":{{\"input_tokens\":7,\"output_tokens\":2}},\"total_cost_usd\":0.000009}}'\nexit 1"
    )
    .unwrap();
    let mut permissions = binary.as_file().metadata().unwrap().permissions();
    permissions.set_mode(0o700);
    binary.as_file().set_permissions(permissions).unwrap();
    // close the write handle before exec: Linux refuses to run a file that is still open for
    // writing (ETXTBSY), and the path stays on disk until this value drops.
    let binary = binary.into_temp_path();

    let provider = AiCommandProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(uuid::Uuid::now_v7()),
        action_name: "ai-command".into(),
        action_function: "claude_code".into(),
        parameters: json!({
            "binary": &*binary,
            "prompt": "anything",
            "output_format": "json"
        }),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
        idempotency_key: None,
        workspace_path: None,
        execution_profile: None,
        credential_injections: Default::default(),
    };
    let events = Arc::new(CapturedEvents::default());
    let result = provider.execute_service(
        request,
        Some(events.clone()),
        runinator_plugin::cancel::CancellationToken::new(),
    );

    assert!(result.is_err());
    assert!(events.0.lock().unwrap().iter().any(|event| matches!(
        event,
        runinator_models::runs::ProviderExecutionEvent::AiUsage { usage }
            if usage.tokens.input_tokens == 7 && usage.provider_cost_microusd == Some(9)
    )));
}

#[test]
fn test_claude_code_invalid_params_missing_prompt() {
    let provider = AiCommandProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(uuid::Uuid::now_v7()),
        action_name: "ai-command".into(),
        action_function: "claude_code".into(),
        parameters: json!({
            "binary": "/bin/echo"
        }),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
        idempotency_key: None,
        workspace_path: None,
        execution_profile: None,
        credential_injections: Default::default(),
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
        run_id: Some(uuid::Uuid::now_v7()),
        action_name: "ai-command".into(),
        action_function: "claude_code".into(),
        parameters: json!({
            "binary": "/bin/echo",
            "model": "claude-opus-5",
            "output_format": "text",
            "prompt": "{\"ok\":true}"
        }),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
        idempotency_key: None,
        workspace_path: None,
        execution_profile: None,
        credential_injections: Default::default(),
    };

    let result = provider
        .execute_service(
            request,
            None,
            runinator_plugin::cancel::CancellationToken::new(),
        )
        .unwrap();
    let text = result.output_json.unwrap()["response"]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("{\"ok\":true}"));
}
