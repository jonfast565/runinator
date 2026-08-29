use std::{
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use log::warn;
use runinator_models::{
    errors::SendableError,
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::cancel::CancellationToken;
use runinator_plugin::provider::ProviderEventSink;
use runinator_provider_support::process::ProcessOutputPump;
use runinator_provider_support::terminal::{self, CommandBuilder, TerminalError};

use crate::errors::{
    CANCELED, INTERACTIVE_NOT_PERMITTED, NONZERO_EXIT, STDERR_UNAVAILABLE, STDOUT_UNAVAILABLE,
    TERMINAL_UNAVAILABLE, TIMEOUT, WORKING_DIR_MISSING,
};
use crate::params::{ConsoleResult, parse_params, to_runtime_error};

// whether `interactive: true` is permitted on this worker, from the `ALLOW_INTERACTIVE_ENV` flag the
// desktop agent sets. a missing, empty, or "0" value means not permitted (the cloud-worker default).
fn interactive_permitted() -> bool {
    allow_interactive(std::env::var(crate::ALLOW_INTERACTIVE_ENV).ok().as_deref())
}

// pure decision split from the env read so it is unit-testable without mutating process env.
fn allow_interactive(raw: Option<&str>) -> bool {
    matches!(raw, Some(value) if !value.is_empty() && value != "0")
}

// the base directory console commands run from, from the `WORKING_DIR_ENV` var the desktop agent
// sets. a missing or empty value means inherit the worker process's cwd (unchanged behavior).
fn configured_working_dir() -> Option<PathBuf> {
    working_dir(std::env::var(crate::WORKING_DIR_ENV).ok().as_deref())
}

// pure decision split from the env read so it is unit-testable without mutating process env.
fn working_dir(raw: Option<&str>) -> Option<PathBuf> {
    match raw {
        Some(value) if !value.trim().is_empty() => Some(PathBuf::from(value.trim())),
        _ => None,
    }
}

// build the shell command for `command_text`, pinning its `current_dir` to the configured working
// directory when one is set so a relative path in the command resolves predictably. surfaces a clear
// error if that directory is configured but missing, rather than letting `spawn` fail obscurely.
fn build_shell_command(
    command_text: &str,
    request: &ProviderExecutionRequest,
) -> Result<Command, SendableError> {
    let mut command = runinator_platform::shell::shell_command(command_text);
    if let Some(dir) = resolved_working_dir(request)? {
        command.current_dir(&dir);
    }
    Ok(command)
}

fn resolved_working_dir(
    request: &ProviderExecutionRequest,
) -> Result<Option<PathBuf>, SendableError> {
    let dir = if request.workspace_path.is_some() {
        runinator_provider_support::resolve_working_dir(request.workspace_path.as_deref(), None)?
    } else {
        configured_working_dir()
    };
    if let Some(dir) = dir {
        if !dir.is_dir() {
            return Err(WORKING_DIR_MISSING.error(dir.display().to_string()));
        }
        return Ok(Some(dir));
    }
    Ok(None)
}

pub(crate) fn execute_command(
    request: &ProviderExecutionRequest,
    sink: Option<Arc<dyn ProviderEventSink>>,
    token: CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    let params = parse_params(request)?;
    let command_text = params.command;
    let started = Instant::now();
    let timeout = Duration::from_secs(request.timeout_secs.max(1) as u64);

    // Interactive commands run in a real platform pseudo-terminal (PTY on Unix, ConPTY on Windows).
    // The child retains normal terminal semantics while its merged terminal stream and input are
    // owned by the worker and relayed to Command Center.
    if params.interactive {
        if !interactive_permitted() {
            return Err(INTERACTIVE_NOT_PERMITTED.error(
                "set this action to run on a desktop worker agent (e.g. `.runner(\"desktop\")`)",
            ));
        }
        return execute_interactive(request, sink, token, command_text, timeout, started);
    }

    let mut command = build_shell_command(&command_text, request)?;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(to_runtime_error)?;
    let output = ProcessOutputPump::start_discarding(&mut child, sink).map_err(|error| {
        if error.to_string().contains("stderr") {
            STDERR_UNAVAILABLE.error(error)
        } else {
            STDOUT_UNAVAILABLE.error(error)
        }
    })?;
    let status = wait_for_child(&mut child, timeout, started, token);
    // always drain to EOF, including after a timeout or cancel killed the child, so tail output is
    // emitted before the worker publishes the terminal result.
    let _ = output.finish();
    let status = status?;

    build_result(status, started, command_text)
}

fn execute_interactive(
    request: &ProviderExecutionRequest,
    sink: Option<Arc<dyn ProviderEventSink>>,
    token: CancellationToken,
    command_text: String,
    timeout: Duration,
    started: Instant,
) -> Result<TaskExecutionResult, SendableError> {
    let mut command = terminal_shell_command(&command_text);
    if let Some(dir) = resolved_working_dir(request)? {
        command.cwd(dir);
    }
    match terminal::run(command, sink, token, timeout) {
        Ok(status) => build_result_parts(status.success, status.exit_code, started, command_text),
        Err(TerminalError::Canceled) => Err(CANCELED.bare()),
        Err(TerminalError::TimedOut(timeout)) => Err(TIMEOUT.error(format!(
            "Console command timed out after {} seconds",
            timeout.as_secs()
        ))),
        Err(error) => Err(TERMINAL_UNAVAILABLE.error(error)),
    }
}

fn terminal_shell_command(command_text: &str) -> CommandBuilder {
    #[cfg(target_os = "windows")]
    {
        let mut command = CommandBuilder::new("cmd");
        command.args(["/C", command_text]);
        command
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut command = CommandBuilder::new("sh");
        command.args(["-c", command_text]);
        command
    }
}

// build the task result from an exited child: success carries the console outcome, a non-zero exit
// surfaces the shared error code. shared by the piped and interactive execution paths.
fn build_result(
    status: ExitStatus,
    started: Instant,
    command_text: String,
) -> Result<TaskExecutionResult, SendableError> {
    let exit_code = status.code().unwrap_or(-1);
    build_result_parts(status.success(), exit_code, started, command_text)
}

fn build_result_parts(
    success: bool,
    exit_code: i32,
    started: Instant,
    command_text: String,
) -> Result<TaskExecutionResult, SendableError> {
    let duration_ms = started.elapsed().as_millis() as i64;
    let result = ConsoleResult {
        success,
        exit_code,
        duration_ms,
        command: command_text,
    };

    if result.success {
        Ok(TaskExecutionResult {
            message: Some(format!("Console command exited with code {exit_code}")),
            output_json: serde_json::to_value(result).ok().map(Into::into),
            chunks: Vec::new(),
            artifacts: Vec::new(),
        })
    } else {
        Err(NONZERO_EXIT.error(format!("exit code {exit_code}")))
    }
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod runner_tests;

fn wait_for_child(
    child: &mut Child,
    timeout: Duration,
    start: Instant,
    token: CancellationToken,
) -> Result<ExitStatus, SendableError> {
    loop {
        if token.is_cancelled() {
            warn!("Console child received cancellation; killing process");
            let _ = child.kill();
            let _ = child.wait();
            return Err(CANCELED.bare());
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    warn!("Console child exceeded timeout; killing process");
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(TIMEOUT.error(format!(
                        "Console command timed out after {} seconds",
                        timeout.as_secs()
                    )));
                }
            }
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(to_runtime_error(err));
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
}
