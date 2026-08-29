use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use runinator_models::json;
use runinator_models::value::Value;
use runinator_models::{
    errors::SendableError,
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::cancel::CancellationToken;
use runinator_plugin::provider::ProviderEventSink;
use runinator_provider_support::process::ProcessOutputPump;

use crate::errors::{
    CLAUDE_CANCELED, CLAUDE_EXIT_CODE, CLAUDE_INVALID_JSON, CLAUDE_SPAWN, CLAUDE_TIMEOUT,
};
use crate::params::{ClaudeCodeParams, parse_params};

pub(crate) fn run_claude_code(
    request: &ProviderExecutionRequest,
    sink: Option<Arc<dyn ProviderEventSink>>,
    token: CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    let params: ClaudeCodeParams = parse_params(request)?;
    if token.is_cancelled() {
        return Err(CLAUDE_CANCELED.bare());
    }
    let argv = build_claude_argv(&params);

    let mut command = Command::new(&params.binary);
    command
        .args(&argv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = runinator_provider_support::resolve_working_dir(
        request.workspace_path.as_deref(),
        params.working_dir.as_deref(),
    )? {
        command.current_dir(dir);
    }
    for (key, value) in &params.env {
        command.env(key, value);
    }

    let mut child = command
        .spawn()
        .map_err(|err| CLAUDE_SPAWN.error(format!("failed to spawn {}: {err}", params.binary)))?;

    let output = ProcessOutputPump::start(&mut child, sink)?;
    let status = wait_for_child(&mut child, request.timeout_secs, token);
    let output = output.finish();
    let status = status?;

    if !status.success() {
        return Err(
            CLAUDE_EXIT_CODE.error(format!("claude exited with {status}: {}", output.stderr))
        );
    }

    let parsed = parse_claude_output(&params.output_format, &output.stdout)?;
    Ok(TaskExecutionResult {
        message: Some("Claude Code completed".into()),
        output_json: Some(parsed),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn wait_for_child(
    child: &mut Child,
    timeout_secs: i64,
    token: CancellationToken,
) -> Result<ExitStatus, SendableError> {
    let timeout = Duration::from_secs(timeout_secs.max(1) as u64);
    let started = Instant::now();
    loop {
        if token.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CLAUDE_CANCELED.bare());
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CLAUDE_TIMEOUT.error(format!(
                "Claude Code timed out after {} seconds",
                timeout.as_secs()
            )));
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Box::new(error));
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn build_claude_argv(params: &ClaudeCodeParams) -> Vec<String> {
    let mut argv = vec![
        "-p".into(),
        "--model".into(),
        params.model.clone(),
        "--output-format".into(),
        params.output_format.clone(),
    ];
    if let Some(tools) = params.allowed_tools.as_deref() {
        argv.push("--allowedTools".into());
        argv.push(tools.into());
    }
    if let Some(mode) = params.permission_mode.as_deref() {
        argv.push("--permission-mode".into());
        argv.push(mode.into());
    }
    for arg in &params.extra_args {
        argv.push(arg.clone());
    }
    // prompt is the trailing positional argument.
    argv.push(params.prompt.clone());
    argv
}

fn parse_claude_output(format: &str, stdout: &str) -> Result<Value, SendableError> {
    match format {
        "json" | "stream-json" => serde_json::from_str::<Value>(stdout).map_err(|err| {
            CLAUDE_INVALID_JSON.error(format!(
                "claude stdout was not valid JSON ({format}): {err}"
            ))
        }),
        _ => Ok(json!({ "text": stdout })),
    }
}
