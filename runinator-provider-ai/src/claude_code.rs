use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use runinator_models::json;
use runinator_models::value::Value;
use runinator_models::{
    errors::SendableError,
    runs::{
        ProviderExecutionEvent, ProviderExecutionRequest, ProviderTerminalControl,
        TaskExecutionResult,
    },
};
use runinator_plugin::cancel::CancellationToken;
use runinator_plugin::provider::ProviderEventSink;
use runinator_provider_support::process_runner::{ProcessFailure, ProcessRequest, ProcessRunner};
use runinator_provider_support::terminal::{self, CommandBuilder, TerminalError};

use crate::errors::{
    CLAUDE_CANCELED, CLAUDE_EXIT_CODE, CLAUDE_INPUT, CLAUDE_INTERACTIVE_NOT_PERMITTED,
    CLAUDE_INVALID_JSON, CLAUDE_SPAWN, CLAUDE_TIMEOUT,
};
use crate::params::{ClaudeCodeParams, parse_params};

const MAX_HARNESS_LINE_BYTES: usize = 1024 * 1024;
const MAX_HARNESS_STDERR_BYTES: usize = 64 * 1024;
const MAX_HARNESS_EVENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_HARNESS_EVENTS: usize = 10_000;

struct HarnessLine {
    stream: String,
    content: String,
    truncated: bool,
}

#[derive(Default)]
struct HarnessEventBudget {
    events: usize,
    bytes: usize,
}

impl HarnessEventBudget {
    fn admit(&mut self, bytes: usize) -> bool {
        if self.events >= MAX_HARNESS_EVENTS
            || self.bytes.saturating_add(bytes) > MAX_HARNESS_EVENT_BYTES
        {
            return false;
        }
        self.events += 1;
        self.bytes += bytes;
        true
    }
}

pub(crate) fn run_claude_code(
    request: &ProviderExecutionRequest,
    sink: Option<Arc<dyn ProviderEventSink>>,
    token: CancellationToken,
    runner: &dyn ProcessRunner,
) -> Result<TaskExecutionResult, SendableError> {
    let params: ClaudeCodeParams = parse_params(request)?;
    if params.mission_mcp && params.mcp_config.is_some() {
        return Err(CLAUDE_INPUT
            .error("mission_mcp and caller-authored mcp_config cannot be enabled together"));
    }
    if params.mission_mcp
        && params
            .mission_id
            .as_deref()
            .and_then(|value| value.parse::<uuid::Uuid>().ok())
            .is_none()
    {
        return Err(CLAUDE_INPUT.error("mission_mcp requires a valid mission_id UUID"));
    }
    if token.is_cancelled() {
        return Err(CLAUDE_CANCELED.bare());
    }
    if params.interactive {
        return run_claude_interactive(request, params, sink, token);
    }
    if params.harnessed {
        return run_claude_harness(request, params, sink, token);
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
    if let Some(profile) = &request.execution_profile {
        if let Some(home) = &profile.home {
            command.env("HOME", home);
        }
        command.envs(&profile.environment);
    }

    let timeout = Duration::from_secs(request.timeout_secs.max(1) as u64);
    let result = runner
        .run(ProcessRequest {
            command: &mut command,
            input: None,
            timeout,
            cancellation: &token,
            sink,
        })
        .map_err(|error| match error {
            ProcessFailure::Canceled => CLAUDE_CANCELED.bare(),
            ProcessFailure::TimedOut => CLAUDE_TIMEOUT.error(format!(
                "Claude Code timed out after {} seconds",
                timeout.as_secs()
            )),
            ProcessFailure::Spawn(error) => {
                CLAUDE_SPAWN.error(format!("failed to spawn {}: {error}", params.binary))
            }
            ProcessFailure::Io(error) => Box::new(error) as SendableError,
        })?;
    let status = result.status;
    let output = result.output;

    if !status.success() {
        return Err(
            CLAUDE_EXIT_CODE.error(format!("claude exited with {status}: {}", output.stderr))
        );
    }

    let parsed = parse_claude_output(&params.output_format, &output.stdout)?;
    Ok(TaskExecutionResult {
        message: Some("Claude Code completed".into()),
        // preserve the advertised `response: any` contract for workflow bindings.
        output_json: Some(json!({ "response": parsed })),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

/// Run Claude Code's documented `stream-json` protocol as a bounded, non-PTY session.
///
/// This gives the durable effect exactly one owner of stdin. The worker's existing terminal
/// control channel becomes safe structured steering: each input is encoded as a Claude Code user
/// message, not written as arbitrary bytes to a shell. The actual phase graph remains owned by
/// Runinator; the agent can only receive additional mission context while its effect is active.
fn run_claude_harness(
    request: &ProviderExecutionRequest,
    params: ClaudeCodeParams,
    sink: Option<Arc<dyn ProviderEventSink>>,
    token: CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    let timeout = Duration::from_secs(request.timeout_secs.max(1) as u64);
    let mut command = Command::new(&params.binary);
    command
        .args(build_claude_harness_argv(&params))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_command(&mut command, request, &params)?;
    let mut child = command.spawn().map_err(|error| {
        CLAUDE_SPAWN.error(format!("failed to spawn {}: {error}", params.binary))
    })?;
    let Some(stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(CLAUDE_SPAWN.error("Claude Code stdin was not available"));
    };
    let mut stdin = Some(stdin);
    let initial_input = stdin
        .as_mut()
        .ok_or_else(|| CLAUDE_INPUT.error("Claude Code stdin closed before initial input"))
        .and_then(|stdin| write_harness_message(stdin, &params.prompt));
    if let Err(error) = initial_input {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }

    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(CLAUDE_SPAWN.error("Claude Code stdout was not available"));
    };
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(CLAUDE_SPAWN.error("Claude Code stderr was not available"));
    };
    // bounded queue applies pipe backpressure when Claude emits faster than durable events can be
    // processed, preventing a high-volume session from becoming an in-memory transcript.
    let (lines, line_receiver) = mpsc::sync_channel(128);
    let stdout_reader = spawn_harness_reader(stdout, "stdout", lines.clone());
    let stderr_reader = spawn_harness_reader(stderr, "stderr", lines);
    let mut terminal = sink.as_ref().and_then(|sink| sink.take_terminal_control());
    let started = Instant::now();
    let mut result = None;
    let mut stderr_output = String::new();
    let mut event_budget = HarnessEventBudget::default();

    let status = loop {
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
        if let Err(error) =
            drain_harness_controls(&mut terminal, &mut stdin, sink.as_ref(), &mut event_budget)
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        match line_receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(line) => {
                handle_harness_line(&line, sink.as_ref(), &mut result, &mut event_budget);
                if line.stream == "stderr" {
                    append_bounded_stderr(&mut stderr_output, &line.content);
                }
                if result.is_some() {
                    // Claude has published its terminal protocol event; closing stdin permits
                    // the subprocess to finish even when it is capable of a later turn. Keep
                    // polling rather than blocking in `wait`, so cancellation and the effect
                    // timeout remain enforceable during a misbehaving client shutdown.
                    stdin.take();
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(status) = child
                    .try_wait()
                    .map_err(|error| Box::new(error) as SendableError)?
                {
                    break status;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                stdin.take();
                if let Some(status) = child
                    .try_wait()
                    .map_err(|error| Box::new(error) as SendableError)?
                {
                    break status;
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
    };
    let drain_started = Instant::now();
    loop {
        match line_receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(line) => {
                handle_harness_line(&line, sink.as_ref(), &mut result, &mut event_budget);
                if line.stream == "stderr" {
                    append_bounded_stderr(&mut stderr_output, &line.content);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout)
                if drain_started.elapsed() >= Duration::from_secs(1) =>
            {
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    drop(line_receiver);
    drop(stdout_reader);
    drop(stderr_reader);
    if !status.success() {
        return Err(CLAUDE_EXIT_CODE.error(format!("claude exited with {status}: {stderr_output}")));
    }
    let Some(response) = result else {
        return Err(CLAUDE_INVALID_JSON
            .error("Claude Code harness finished without a terminal stream-json result event"));
    };
    if response
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let message = response
            .get("result")
            .and_then(Value::as_str)
            .unwrap_or("Claude Code reported an unsuccessful result");
        return Err(
            CLAUDE_EXIT_CODE.error(format!("Claude Code harness reported failure: {message}"))
        );
    }
    let session_id = response
        .get("session_id")
        .or_else(|| response.get("sessionId"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(TaskExecutionResult {
        message: Some(match params.role.as_deref() {
            Some(role) => format!("Claude Code {role} session completed"),
            None => "Claude Code harness session completed".into(),
        }),
        output_json: Some(json!({
            "response": response,
            "harness": {
                "role": params.role,
                "session_id": session_id,
            },
        })),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn configure_command(
    command: &mut Command,
    request: &ProviderExecutionRequest,
    params: &ClaudeCodeParams,
) -> Result<(), SendableError> {
    if let Some(dir) = runinator_provider_support::resolve_working_dir(
        request.workspace_path.as_deref(),
        params.working_dir.as_deref(),
    )? {
        command.current_dir(dir);
    }
    for (key, value) in &params.env {
        command.env(key, value);
    }
    if let Some(profile) = &request.execution_profile {
        if let Some(home) = &profile.home {
            command.env("HOME", home);
        }
        command.envs(&profile.environment);
    }
    Ok(())
}

fn write_harness_message(stdin: &mut impl Write, text: &str) -> Result<(), SendableError> {
    let message = serde_json::to_string(&json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{ "type": "text", "text": text }],
        },
    }))
    .map_err(|error| CLAUDE_INPUT.error(format!("could not encode Claude Code input: {error}")))?;
    stdin
        .write_all(message.as_bytes())
        .and_then(|()| stdin.write_all(b"\n"))
        .and_then(|()| stdin.flush())
        .map_err(|error| CLAUDE_INPUT.error(format!("could not write Claude Code input: {error}")))
}

fn spawn_harness_reader<R>(
    reader: R,
    stream: &'static str,
    sender: mpsc::SyncSender<HarnessLine>,
) -> thread::JoinHandle<()>
where
    R: std::io::Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        while let Ok(Some((content, truncated))) =
            read_bounded_line(&mut reader, MAX_HARNESS_LINE_BYTES)
        {
            if sender
                .send(HarnessLine {
                    stream: stream.into(),
                    content,
                    truncated,
                })
                .is_err()
            {
                break;
            }
        }
    })
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    max_bytes: usize,
) -> std::io::Result<Option<(String, bool)>> {
    let mut output = Vec::with_capacity(max_bytes.min(8 * 1024));
    let mut saw_bytes = false;
    let mut truncated = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if !saw_bytes {
                return Ok(None);
            }
            break;
        }
        saw_bytes = true;
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        let content_end = newline.unwrap_or(available.len());
        let remaining = max_bytes.saturating_sub(output.len());
        let copied = remaining.min(content_end);
        output.extend_from_slice(&available[..copied]);
        truncated |= copied < content_end;
        reader.consume(consumed);
        if newline.is_some() {
            break;
        }
    }
    if output.last() == Some(&b'\r') {
        output.pop();
    }
    let mut output = String::from_utf8_lossy(&output).into_owned();
    if truncated {
        output.push_str("…[truncated]");
    }
    Ok(Some((output, truncated)))
}

fn append_bounded_stderr(output: &mut String, line: &str) {
    if output.len() >= MAX_HARNESS_STDERR_BYTES {
        return;
    }
    let remaining = MAX_HARNESS_STDERR_BYTES - output.len();
    let mut copied = line.len().min(remaining);
    while !line.is_char_boundary(copied) {
        copied -= 1;
    }
    output.push_str(&line[..copied]);
    if copied < line.len() {
        return;
    }
    if output.len() < MAX_HARNESS_STDERR_BYTES {
        output.push('\n');
    }
}

fn drain_harness_controls<W: Write>(
    terminal: &mut Option<mpsc::Receiver<ProviderTerminalControl>>,
    stdin: &mut Option<W>,
    sink: Option<&Arc<dyn ProviderEventSink>>,
    budget: &mut HarnessEventBudget,
) -> Result<(), SendableError> {
    let Some(receiver) = terminal.as_ref() else {
        return Ok(());
    };
    loop {
        match receiver.try_recv() {
            Ok(ProviderTerminalControl::Input { data }) => {
                let Some(stdin) = stdin.as_mut() else {
                    return Err(CLAUDE_INPUT
                        .error("Claude Code session input is closed and cannot accept steering"));
                };
                write_harness_message(stdin, &data)?;
                // accepted steering is audit input, not subprocess output; never discard it when
                // a noisy Claude session exhausts its output-event budget.
                emit_progress(
                    sink,
                    "claude.steering",
                    json!({ "bytes": data.len(), "message": data }),
                );
            }
            Ok(ProviderTerminalControl::Eof) => {
                if budget.admit(0) {
                    emit_progress(sink, "claude.steering_closed", json!({}));
                }
                stdin.take();
                *terminal = None;
                return Ok(());
            }
            Ok(ProviderTerminalControl::Resize { cols, rows }) => {
                if budget.admit(0) {
                    emit_progress(
                        sink,
                        "claude.resize_ignored",
                        json!({ "cols": cols, "rows": rows }),
                    );
                }
            }
            Err(mpsc::TryRecvError::Empty) | Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
        }
    }
}

fn handle_harness_line(
    line: &HarnessLine,
    sink: Option<&Arc<dyn ProviderEventSink>>,
    result: &mut Option<Value>,
    budget: &mut HarnessEventBudget,
) {
    if line.stream != "stdout" {
        if budget.admit(line.content.len())
            && let Some(sink) = sink
        {
            sink.emit(ProviderExecutionEvent::Chunk {
                stream: line.stream.clone(),
                content: line.content.clone(),
            });
        }
        return;
    }
    if line.truncated {
        if budget.admit(line.content.len())
            && let Some(sink) = sink
        {
            sink.emit(ProviderExecutionEvent::Chunk {
                stream: line.stream.clone(),
                content: line.content.clone(),
            });
        }
        return;
    }
    let Ok(event) = serde_json::from_str::<Value>(&line.content) else {
        if budget.admit(line.content.len())
            && let Some(sink) = sink
        {
            sink.emit(ProviderExecutionEvent::Chunk {
                stream: line.stream.clone(),
                content: line.content.clone(),
            });
        }
        return;
    };
    let kind = event.get("type").and_then(Value::as_str).unwrap_or("event");
    if budget.admit(line.content.len()) {
        emit_progress(sink, &format!("claude.{kind}"), event.clone());
    }
    if kind == "result" {
        *result = Some(event);
    }
}

fn emit_progress(sink: Option<&Arc<dyn ProviderEventSink>>, kind: &str, payload: Value) {
    if let Some(sink) = sink {
        sink.emit(ProviderExecutionEvent::Progress {
            kind: kind.into(),
            payload,
        });
    }
}

fn run_claude_interactive(
    request: &ProviderExecutionRequest,
    params: ClaudeCodeParams,
    sink: Option<Arc<dyn ProviderEventSink>>,
    token: CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    if !terminal::interactive_permitted() {
        return Err(CLAUDE_INTERACTIVE_NOT_PERMITTED.error(
            "route this action to a desktop worker (for example with `.runner(\"desktop\")`)",
        ));
    }
    let mut command = CommandBuilder::new(&params.binary);
    command.args(build_claude_interactive_argv(&params));
    if let Some(dir) = runinator_provider_support::resolve_working_dir(
        request.workspace_path.as_deref(),
        params.working_dir.as_deref(),
    )? {
        command.cwd(dir);
    }
    for (key, value) in &params.env {
        command.env(key, value);
    }
    if let Some(profile) = &request.execution_profile {
        if let Some(home) = &profile.home {
            command.env("HOME", home);
        }
        for (key, value) in &profile.environment {
            command.env(key, value);
        }
    }
    let timeout = Duration::from_secs(request.timeout_secs.max(1) as u64);
    let status = match terminal::run(command, sink, token, timeout) {
        Ok(status) => status,
        Err(TerminalError::Canceled) => return Err(CLAUDE_CANCELED.bare()),
        Err(TerminalError::TimedOut(timeout)) => {
            return Err(CLAUDE_TIMEOUT.error(format!(
                "Claude Code timed out after {} seconds",
                timeout.as_secs()
            )));
        }
        Err(error) => return Err(CLAUDE_SPAWN.error(error)),
    };
    if !status.success {
        return Err(CLAUDE_EXIT_CODE.error(format!("claude exited with code {}", status.exit_code)));
    }
    Ok(TaskExecutionResult {
        message: Some("Interactive Claude Code session completed".into()),
        output_json: Some(json!({
            "interactive": true,
            "exit_code": status.exit_code,
        })),
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

fn build_claude_harness_argv(params: &ClaudeCodeParams) -> Vec<String> {
    let mut argv = vec![
        "-p".into(),
        "--model".into(),
        params.model.clone(),
        "--input-format".into(),
        "stream-json".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
    ];
    if let Some(session) = params.resume_session.as_deref() {
        argv.push("--resume".into());
        argv.push(session.into());
    }
    if let Some(path) = params.mcp_config.as_deref() {
        argv.push("--mcp-config".into());
        argv.push(path.into());
    }
    if params.mission_mcp {
        argv.push("--mcp-config".into());
        argv.push(mission_mcp_config(params));
        argv.push("--strict-mcp-config".into());
        argv.push("--restricted".into());
        argv.push("--permission-prompts".into());
        argv.push("none".into());
    }
    if let Some(max_turns) = params.max_turns {
        argv.push("--max-turns".into());
        argv.push(max_turns.to_string());
    }
    if let Some(tools) = params.allowed_tools.as_deref() {
        argv.push("--allowedTools".into());
        argv.push(tools.into());
    }
    if let Some(mode) = params.permission_mode.as_deref() {
        argv.push("--permission-mode".into());
        argv.push(mode.into());
    }
    argv.extend(params.extra_args.iter().cloned());
    argv
}

fn mission_mcp_config(params: &ClaudeCodeParams) -> String {
    json!({
        "mcpServers": {
            "runinator-mission": {
                "command": "runinatorctl",
                "args": ["mcp", "--mission-only", "--mission-id", params.mission_id],
            }
        }
    })
    .to_string()
}

fn build_claude_interactive_argv(params: &ClaudeCodeParams) -> Vec<String> {
    let mut argv = vec!["--model".into(), params.model.clone()];
    if let Some(tools) = params.allowed_tools.as_deref() {
        argv.push("--allowedTools".into());
        argv.push(tools.into());
    }
    if let Some(mode) = params.permission_mode.as_deref() {
        argv.push("--permission-mode".into());
        argv.push(mode.into());
    }
    argv.extend(params.extra_args.iter().cloned());
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

#[cfg(test)]
#[path = "claude_runner_tests.rs"]
mod runner_tests;
