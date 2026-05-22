use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use runinator_models::{
    errors::{RuntimeError, SendableError},
    runs::{ProviderExecutionEvent, ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::ProviderEventSink;
use serde_json::{Value, json};

use crate::params::{ClaudeCodeParams, parse_params};

pub(crate) fn run_claude_code(
    request: &ProviderExecutionRequest,
    sink: Option<Arc<dyn ProviderEventSink>>,
) -> Result<TaskExecutionResult, SendableError> {
    let params: ClaudeCodeParams = parse_params(request)?;
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
    let output = child.wait_with_output().map_err(|err| {
        RuntimeError::new(
            "ai_command.claude_code.wait".into(),
            format!("failed to wait for claude: {err}"),
        )
    })?;
    let stdout = stdout_handle
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_else(|| String::from_utf8_lossy(&output.stdout).into_owned());

    if !output.status.success() {
        return Err(Box::new(RuntimeError::new(
            "ai_command.claude_code.exit_code".into(),
            format!(
                "claude exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ),
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
