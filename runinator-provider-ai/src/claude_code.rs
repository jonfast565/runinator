use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use runinator_models::{
    errors::{RuntimeError, SendableError},
    runs::{ProviderExecutionEvent, ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::cancel::CancellationToken;
use runinator_plugin::provider::ProviderEventSink;
use serde_json::{Value, json};

use crate::params::{ClaudeCodeParams, parse_params};

pub(crate) fn run_claude_code(
    request: &ProviderExecutionRequest,
    sink: Option<Arc<dyn ProviderEventSink>>,
    token: CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    let params: ClaudeCodeParams = parse_params(request)?;
    if token.is_cancelled() {
        return Err(Box::new(RuntimeError::new(
            "ai_command.claude_code.canceled".into(),
            "Claude Code command canceled".into(),
        )));
    }
    let argv = build_claude_argv(&params);

    let mut command = Command::new(&params.binary);
    command
        .args(&argv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = params.working_dir.as_deref() {
        command.current_dir(dir);
    }
    for (key, value) in &params.env {
        command.env(key, value);
    }

    let mut child = command.spawn().map_err(|err| {
        RuntimeError::new(
            "ai_command.claude_code.spawn".into(),
            format!("failed to spawn {}: {err}", params.binary),
        )
    })?;

    let stdout_handle = drain_stdout(&mut child, sink.as_ref());
    let stderr_handle = drain_stderr(&mut child);
    if token.is_cancelled() {
        let _ = child.kill();
        return Err(Box::new(RuntimeError::new(
            "ai_command.claude_code.canceled".into(),
            "Claude Code command canceled".into(),
        )));
    }
    let status = wait_for_child(&mut child, request.timeout_secs, token)?;
    let stdout = stdout_handle
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr = stderr_handle
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_default();

    if !status.success() {
        return Err(Box::new(RuntimeError::new(
            "ai_command.claude_code.exit_code".into(),
            format!("claude exited with {status}: {stderr}"),
        )));
    }

    let parsed = parse_claude_output(&params.output_format, &stdout)?;
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
            return Err(Box::new(RuntimeError::new(
                "ai_command.claude_code.canceled".into(),
                "Claude Code command canceled".into(),
            )));
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Box::new(RuntimeError::new(
                "ai_command.claude_code.timeout".into(),
                format!("Claude Code timed out after {} seconds", timeout.as_secs()),
            )));
        }
        if let Some(status) = child.try_wait()? {
            return Ok(status);
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

fn drain_stdout(
    child: &mut Child,
    sink: Option<&Arc<dyn ProviderEventSink>>,
) -> Option<JoinHandle<String>> {
    let stdout: ChildStdout = child.stdout.take()?;
    let sink = sink.cloned();
    let handle = thread::spawn(move || {
        let mut accumulator = String::new();
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if let Some(sink) = sink.as_ref() {
                sink.emit(ProviderExecutionEvent::Chunk {
                    stream: "stdout".into(),
                    content: format!("{line}\n"),
                });
            }
            accumulator.push_str(&line);
            accumulator.push('\n');
        }
        accumulator
    });
    Some(handle)
}

fn drain_stderr(child: &mut Child) -> Option<JoinHandle<String>> {
    let stderr: ChildStderr = child.stderr.take()?;
    let handle = thread::spawn(move || {
        let mut accumulator = String::new();
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            accumulator.push_str(&line);
            accumulator.push('\n');
        }
        accumulator
    });
    Some(handle)
}

fn parse_claude_output(format: &str, stdout: &str) -> Result<Value, SendableError> {
    match format {
        "json" | "stream-json" => serde_json::from_str::<Value>(stdout).map_err(|err| {
            Box::new(RuntimeError::new(
                "ai_command.claude_code.invalid_json".into(),
                format!("claude stdout was not valid JSON ({format}): {err}"),
            )) as SendableError
        }),
        _ => Ok(json!({ "text": stdout })),
    }
}
