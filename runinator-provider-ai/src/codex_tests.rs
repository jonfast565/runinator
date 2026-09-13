//! Codex CLI protocol and mission-boundary tests.

use super::*;

#[test]
fn exec_jsonl_returns_the_last_agent_message_and_usage() {
    let parsed = parse_exec_jsonl(
        r#"{"type":"thread.started","thread_id":"thread-1"}
{"type":"item.completed","item":{"type":"agent_message","text":"done"}}
{"type":"turn.completed","usage":{"input_tokens":4,"output_tokens":2}}"#,
    )
    .unwrap();
    assert_eq!(parsed.thread_id.as_deref(), Some("thread-1"));
    assert_eq!(parsed.result, "done");
    assert_eq!(
        parsed
            .usage
            .pointer("/output_tokens")
            .and_then(Value::as_i64),
        Some(2)
    );
}

#[test]
fn exec_jsonl_requires_a_terminal_agent_message() {
    let error =
        parse_exec_jsonl(r#"{"type":"thread.started","thread_id":"thread-1"}"#).unwrap_err();
    assert!(error.to_string().contains("without an agent message"));
}

#[test]
fn workspace_write_policy_is_fenced_and_offline() {
    let policy = sandbox_policy("workspace_write", Some(Path::new("/tmp/workspace")));
    assert_eq!(
        policy.pointer("/type").and_then(Value::as_str),
        Some("workspaceWrite")
    );
    assert_eq!(
        policy.pointer("/networkAccess").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        policy.pointer("/writableRoots/0").and_then(Value::as_str),
        Some("/tmp/workspace")
    );
}

#[test]
fn persistent_home_keeps_thread_state_but_scrubs_credentials() {
    let workspace = tempfile::tempdir().unwrap();
    fs::create_dir(workspace.path().join(".git")).unwrap();
    let profile = tempfile::tempdir().unwrap();
    fs::create_dir(profile.path().join(".codex")).unwrap();
    fs::write(profile.path().join(".codex/auth.json"), "secret").unwrap();
    let request = ProviderExecutionRequest {
        run_id: None,
        action_name: "codex".into(),
        action_function: "codex".into(),
        parameters: Value::Null,
        timeout_secs: 30,
        artifact_dir: String::new(),
        events_jsonl_path: String::new(),
        idempotency_key: None,
        workspace_path: Some(workspace.path().to_string_lossy().into_owned()),
        execution_profile: Some(
            runinator_models::execution_profiles::MaterializedExecutionProfile {
                profile_id: uuid::Uuid::now_v7(),
                revision: 1,
                root: profile.path().to_string_lossy().into_owned(),
                home: Some(profile.path().to_string_lossy().into_owned()),
                environment: Default::default(),
            },
        ),
        credential_injections: Default::default(),
    };
    let params: CodexParams = serde_json::from_value(serde_json::json!({
        "prompt": "test",
        "harnessed": true,
        "session_slot": "reviewer",
        "mission_mcp": true,
        "mission_id": "018f5f7c-4b74-7f44-8fd1-cde6b5c4d111"
    }))
    .unwrap();
    let mut home = CodexHome::prepare(&request, &params).unwrap();
    assert!(home.path.join("auth.json").is_file());
    assert!(home.path.join("config.toml").is_file());
    home.save_thread("thread-1").unwrap();
    home.scrub();
    assert!(!home.path.join("auth.json").exists());
    assert!(!home.path.join("config.toml").exists());
    assert_eq!(home.load_thread().unwrap().as_deref(), Some("thread-1"));
}
