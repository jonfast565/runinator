use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, mpsc},
    time::Duration,
};

use runinator_models::{
    errors::SendableError,
    json,
    runs::{ProviderExecutionRequest, TaskExecutionResult},
    value::Value,
};

mod protocol;

use protocol::{AppSession, agent_message_text, send_notification, send_request, spawn_reader};
use runinator_plugin::{cancel::CancellationToken, provider::ProviderEventSink};
use runinator_provider_support::process_runner::{ProcessFailure, ProcessRequest, ProcessRunner};

use crate::{
    errors::{
        CODEX_CANCELED, CODEX_EXIT_CODE, CODEX_INPUT, CODEX_PROTOCOL, CODEX_SPAWN, CODEX_TIMEOUT,
    },
    params::{CodexParams, parse_params},
    usage::{codex_usage, emit as emit_usage},
};

const MAX_LINE_BYTES: usize = 1024 * 1024;
const MAX_EVENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_EVENTS: usize = 10_000;
const MAX_STDERR_BYTES: usize = 64 * 1024;

pub(crate) fn run_codex(
    request: &ProviderExecutionRequest,
    sink: Option<Arc<dyn ProviderEventSink>>,
    token: CancellationToken,
    runner: &dyn ProcessRunner,
) -> Result<TaskExecutionResult, SendableError> {
    let params: CodexParams = parse_params(request)?;
    validate_params(&params, request)?;
    if token.is_cancelled() {
        return Err(CODEX_CANCELED.bare());
    }
    if params.harnessed {
        run_app_server(request, params, sink, token)
    } else {
        run_exec(request, &params, sink, token, runner)
    }
}

fn validate_params(
    params: &CodexParams,
    request: &ProviderExecutionRequest,
) -> Result<(), SendableError> {
    if !matches!(params.sandbox.as_str(), "read_only" | "workspace_write") {
        return Err(CODEX_INPUT.error("sandbox must be read_only or workspace_write"));
    }
    if params.resume_thread.is_some() && params.session_slot.is_some() {
        return Err(CODEX_INPUT.error("resume_thread and session_slot are mutually exclusive"));
    }
    if params.session_slot.is_some() && request.workspace_path.is_none() {
        return Err(CODEX_INPUT.error("session_slot requires a portable workspace"));
    }
    if params.mission_mcp {
        if !params.harnessed {
            return Err(CODEX_INPUT.error("mission_mcp requires harnessed mode"));
        }
        if !params.extra_args.is_empty() {
            return Err(CODEX_INPUT.error("extra_args are not allowed in mission mode"));
        }
        if params
            .mission_id
            .as_deref()
            .and_then(|value| value.parse::<uuid::Uuid>().ok())
            .is_none()
        {
            return Err(CODEX_INPUT.error("mission_mcp requires a valid mission_id UUID"));
        }
    }
    if let Some(slot) = params.session_slot.as_deref()
        && (slot.is_empty()
            || slot.len() > 64
            || !slot
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
    {
        return Err(
            CODEX_INPUT.error("session_slot must be 1-64 ASCII letters, digits, '-' or '_'")
        );
    }
    Ok(())
}

fn run_exec(
    request: &ProviderExecutionRequest,
    params: &CodexParams,
    sink: Option<Arc<dyn ProviderEventSink>>,
    token: CancellationToken,
    runner: &dyn ProcessRunner,
) -> Result<TaskExecutionResult, SendableError> {
    let mut home = CodexHome::prepare(request, params)?;
    let mut command = Command::new(&params.binary);
    command.args(isolated_global_args()).args([
        "exec",
        "--json",
        "--ignore-user-config",
        "--sandbox",
        cli_sandbox(&params.sandbox),
    ]);
    if let Some(model) = params.model.as_deref() {
        command.args(["--model", model]);
    }
    if let Some(effort) = params.reasoning_effort.as_deref() {
        command.args(["-c", &format!("model_reasoning_effort={effort:?}")]);
    }
    let mut schema_file = None;
    if let Some(schema) = &params.output_schema {
        let mut file = tempfile::NamedTempFile::new().map_err(|error| CODEX_INPUT.error(error))?;
        serde_json::to_writer(&mut file, schema).map_err(|error| CODEX_INPUT.error(error))?;
        let path = file
            .path()
            .to_str()
            .ok_or_else(|| CODEX_INPUT.error("output schema path is not valid UTF-8"))?;
        command.args(["--output-schema", path]);
        schema_file = Some(file);
    }
    command.args(&params.extra_args).arg(&params.prompt);
    configure_command(&mut command, request, params.working_dir.as_deref())?;
    command.env("CODEX_HOME", &home.path);
    let timeout = Duration::from_secs(request.timeout_secs.max(1) as u64);
    let result = runner
        .run(ProcessRequest {
            command: &mut command,
            input: None,
            timeout,
            cancellation: &token,
            sink: sink.clone(),
        })
        .map_err(|error| map_process_error(error, timeout, &params.binary))?;
    let parsed = parse_exec_jsonl(&result.output.stdout);
    if let Ok(parsed) = &parsed {
        emit_usage(
            sink.as_ref(),
            codex_usage(&parsed.usage, params.model.as_deref()),
        );
    }
    if !result.status.success() {
        return Err(CODEX_EXIT_CODE.error(format!(
            "codex exited with {}: {}",
            result.status, result.output.stderr
        )));
    }
    let parsed = parsed?;
    drop(schema_file);
    home.scrub();
    Ok(TaskExecutionResult {
        message: Some("Codex completed".into()),
        output_json: Some(json!({
            "response": { "result": parsed.result },
            "harness": {
                "role": params.role,
                "thread_id": parsed.thread_id,
                "turn_id": Value::Null,
                "usage": parsed.usage,
            }
        })),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn parse_exec_jsonl(output: &str) -> Result<ExecResult, SendableError> {
    let mut result = None;
    let mut thread_id = None;
    let mut usage = Value::Null;
    let mut bytes = 0usize;
    for (index, line) in output.lines().enumerate() {
        if index >= MAX_EVENTS || line.len() > MAX_LINE_BYTES {
            return Err(
                CODEX_PROTOCOL.error("Codex JSONL output exceeded its bounded event limits")
            );
        }
        bytes = bytes.saturating_add(line.len());
        if bytes > MAX_EVENT_BYTES {
            return Err(CODEX_PROTOCOL.error("Codex JSONL output exceeded its bounded byte limit"));
        }
        let event: Value = serde_json::from_str(line)
            .map_err(|error| CODEX_PROTOCOL.error(format!("invalid Codex JSONL event: {error}")))?;
        match event.get("type").and_then(Value::as_str) {
            Some("thread.started") => {
                thread_id = event
                    .get("thread_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            Some("item.completed") => {
                if let Some(text) = agent_message_text(event.get("item")) {
                    result = Some(text);
                }
            }
            Some("turn.completed") => usage = event.get("usage").cloned().unwrap_or(Value::Null),
            Some("turn.failed") => {
                return Err(CODEX_EXIT_CODE.error(
                    event
                        .get("error")
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "Codex turn failed".into()),
                ));
            }
            _ => {}
        }
    }
    Ok(ExecResult {
        result: result
            .ok_or_else(|| CODEX_PROTOCOL.error("Codex completed without an agent message"))?,
        thread_id,
        usage,
    })
}

fn run_app_server(
    request: &ProviderExecutionRequest,
    params: CodexParams,
    sink: Option<Arc<dyn ProviderEventSink>>,
    token: CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    let working_dir = runinator_provider_support::resolve_working_dir(
        request.workspace_path.as_deref(),
        params.working_dir.as_deref(),
    )?;
    let mut home = CodexHome::prepare(request, &params)?;
    let mut command = Command::new(&params.binary);
    command.args(isolated_global_args()).args([
        "app-server",
        "--strict-config",
        "--listen",
        "stdio://",
    ]);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = working_dir.as_deref() {
        command.current_dir(dir);
    }
    command.env("CODEX_HOME", &home.path);
    if let Some(profile) = &request.execution_profile {
        command.envs(&profile.environment);
    }
    runinator_provider_support::apply_command_credentials(&mut command, request);
    let mut child = ChildGuard(command.spawn().map_err(|error| {
        CODEX_SPAWN.error(format!("failed to spawn {}: {error}", params.binary))
    })?);
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| CODEX_SPAWN.error("Codex stdin unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CODEX_SPAWN.error("Codex stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| CODEX_SPAWN.error("Codex stderr unavailable"))?;
    let (sender, receiver) = mpsc::sync_channel(128);
    let _stdout_reader = spawn_reader(stdout, "stdout", sender.clone());
    let _stderr_reader = spawn_reader(stderr, "stderr", sender);
    let mut session = AppSession::new(sink.clone(), request.timeout_secs, token);

    send_request(
        &mut stdin,
        1,
        "initialize",
        json!({
            "clientInfo": { "name": "runinator", "title": "Runinator", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": { "experimentalApi": true },
        }),
    )?;
    session.wait_response(&mut child, &mut stdin, &receiver, 1)?;
    send_notification(&mut stdin, "initialized", json!({}))?;

    let saved_thread = home.load_thread()?;
    let resume_thread = params.resume_thread.clone().or(saved_thread);
    let cwd = working_dir
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned());
    let method = if resume_thread.is_some() {
        "thread/resume"
    } else {
        "thread/start"
    };
    let thread_params = match resume_thread {
        Some(thread_id) => json!({
            "threadId": thread_id,
            "cwd": cwd,
            "model": params.model,
            "approvalPolicy": "never",
            "sandbox": cli_sandbox(&params.sandbox),
            "runtimeWorkspaceRoots": working_dir.as_ref().map(|path| vec![path.to_string_lossy().into_owned()]),
        }),
        None => json!({
            "cwd": cwd,
            "model": params.model,
            "approvalPolicy": "never",
            "sandbox": cli_sandbox(&params.sandbox),
            "runtimeWorkspaceRoots": working_dir.as_ref().map(|path| vec![path.to_string_lossy().into_owned()]),
            "serviceName": "runinator",
        }),
    };
    send_request(&mut stdin, 2, method, thread_params)?;
    let thread_response = session.wait_response(&mut child, &mut stdin, &receiver, 2)?;
    let thread_id = thread_response
        .pointer("/result/thread/id")
        .and_then(Value::as_str)
        .ok_or_else(|| CODEX_PROTOCOL.error("Codex thread response did not include a thread id"))?
        .to_owned();
    home.save_thread(&thread_id)?;

    let sandbox_policy = sandbox_policy(&params.sandbox, working_dir.as_deref());
    send_request(
        &mut stdin,
        3,
        "turn/start",
        json!({
            "threadId": thread_id,
            "input": [{ "type": "text", "text": params.prompt }],
            "approvalPolicy": "never",
            "sandboxPolicy": sandbox_policy,
            "model": params.model,
            "effort": params.reasoning_effort,
            "outputSchema": params.output_schema,
            "cwd": cwd,
        }),
    )?;
    let turn_response = session.wait_response(&mut child, &mut stdin, &receiver, 3)?;
    session.turn_id = turn_response
        .pointer("/result/turn/id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let completed = session.run_turn(&mut child, &mut stdin, &receiver, &thread_id)?;
    emit_usage(
        sink.as_ref(),
        codex_usage(&completed.usage, params.model.as_deref()),
    );
    let _ = send_request(
        &mut stdin,
        4,
        "thread/unsubscribe",
        json!({ "threadId": thread_id }),
    );
    let _ = child.kill();
    let _ = child.wait();
    home.scrub();
    Ok(TaskExecutionResult {
        message: Some(match params.role.as_deref() {
            Some(role) => format!("Codex {role} session completed"),
            None => "Codex app-server session completed".into(),
        }),
        output_json: Some(json!({
            "response": { "result": completed.result },
            "harness": {
                "role": params.role,
                "thread_id": thread_id,
                "turn_id": session.turn_id,
                "usage": completed.usage,
            }
        })),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn configure_command(
    command: &mut Command,
    request: &ProviderExecutionRequest,
    working_dir: Option<&str>,
) -> Result<(), SendableError> {
    if let Some(dir) = runinator_provider_support::resolve_working_dir(
        request.workspace_path.as_deref(),
        working_dir,
    )? {
        command.current_dir(dir);
    }
    if let Some(profile) = &request.execution_profile {
        if let Some(home) = &profile.home {
            command.env("HOME", home);
        }
        command.envs(&profile.environment);
    }
    runinator_provider_support::apply_command_credentials(command, request);
    Ok(())
}

fn cli_sandbox(value: &str) -> &'static str {
    match value {
        "workspace_write" => "workspace-write",
        _ => "read-only",
    }
}

fn isolated_global_args() -> [&'static str; 12] {
    [
        "--disable",
        "plugins",
        "--disable",
        "remote_plugin",
        "--disable",
        "plugin_sharing",
        "--disable",
        "skill_search",
        "--disable",
        "multi_agent",
        "--disable",
        "multi_agent_v2",
    ]
}

fn sandbox_policy(value: &str, workspace: Option<&Path>) -> Value {
    match value {
        "workspace_write" => json!({
            "type": "workspaceWrite",
            "writableRoots": workspace.map(|path| vec![path.to_string_lossy().into_owned()]).unwrap_or_default(),
            "networkAccess": false,
        }),
        _ => json!({ "type": "readOnly", "networkAccess": false }),
    }
}

fn map_process_error(error: ProcessFailure, timeout: Duration, binary: &str) -> SendableError {
    match error {
        ProcessFailure::Canceled => CODEX_CANCELED.bare(),
        ProcessFailure::TimedOut => CODEX_TIMEOUT.error(format!(
            "Codex timed out after {} seconds",
            timeout.as_secs()
        )),
        ProcessFailure::Spawn(error) => {
            CODEX_SPAWN.error(format!("failed to spawn {binary}: {error}"))
        }
        ProcessFailure::Io(error) => Box::new(error),
    }
}

#[cfg(test)]
#[path = "codex_tests.rs"]
mod tests;

mod exec_result;
use exec_result::ExecResult;

mod completed_turn;
use completed_turn::CompletedTurn;

mod child_guard;
use child_guard::ChildGuard;

mod codex_home;
use codex_home::CodexHome;
