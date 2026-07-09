use std::{
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use log::warn;
use runinator_models::{
    errors::SendableError,
    runs::{ProviderExecutionEvent, ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::cancel::CancellationToken;
use runinator_plugin::provider::ProviderEventSink;

use crate::errors::{
    CANCELED, INTERACTIVE_NOT_PERMITTED, NONZERO_EXIT, STDERR_UNAVAILABLE, STDOUT_UNAVAILABLE,
    TIMEOUT, WORKING_DIR_MISSING,
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
fn build_shell_command(command_text: &str) -> Result<Command, SendableError> {
    let mut command = runinator_utilities::shell::shell_command(command_text);
    if let Some(dir) = configured_working_dir() {
        if !dir.is_dir() {
            return Err(WORKING_DIR_MISSING.error(dir.display().to_string()));
        }
        command.current_dir(&dir);
    }
    Ok(command)
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

    // interactive mode inherits the worker's stdio so the command runs in the operator's desktop
    // session and can present its own prompts (a browser-based `aws sso login`, a Keychain access
    // dialog). there is no piped output to stream, so this path skips the reader threads. gated to
    // workers that advertise an interactive desktop session: a headless cloud worker has no terminal
    // to attach, so it rejects the request instead of hanging or failing obscurely.
    if params.interactive {
        if !interactive_permitted() {
            return Err(INTERACTIVE_NOT_PERMITTED.error(
                "set this action to run on a desktop worker agent (e.g. `.runner(\"creds-sync\")`)",
            ));
        }
        let mut command = build_shell_command(&command_text)?;
        command
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let mut child = command.spawn().map_err(to_runtime_error)?;
        let status = wait_for_child(&mut child, timeout, started, token)?;
        return build_result(status, started, command_text);
    }

    let mut command = build_shell_command(&command_text)?;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(to_runtime_error)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| STDOUT_UNAVAILABLE.bare())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| STDERR_UNAVAILABLE.bare())?;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stdout_thread = spawn_output_thread(stdout, Arc::clone(&stop_flag), "stdout", sink.clone());
    let stderr_thread = spawn_output_thread(stderr, Arc::clone(&stop_flag), "stderr", sink);
    let status = wait_for_child(&mut child, timeout, started, token)?;

    stop_flag.store(true, Ordering::Relaxed);
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

    build_result(status, started, command_text)
}

// build the task result from an exited child: success carries the console outcome, a non-zero exit
// surfaces the shared error code. shared by the piped and interactive execution paths.
fn build_result(
    status: ExitStatus,
    started: Instant,
    command_text: String,
) -> Result<TaskExecutionResult, SendableError> {
    let exit_code = status.code().unwrap_or(-1);
    let duration_ms = started.elapsed().as_millis() as i64;
    let result = ConsoleResult {
        success: status.success(),
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

fn spawn_output_thread<R: std::io::Read + Send + 'static>(
    reader: R,
    stop_flag: Arc<AtomicBool>,
    stream: &'static str,
    sink: Option<Arc<dyn ProviderEventSink>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let buf_reader = BufReader::new(reader);
        for line in buf_reader.lines() {
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
            match line {
                Ok(content) => {
                    if let Some(sink) = &sink {
                        sink.emit(ProviderExecutionEvent::Chunk {
                            stream: stream.to_string(),
                            content,
                        });
                    }
                }
                Err(err) => {
                    if let Some(sink) = &sink {
                        sink.emit(ProviderExecutionEvent::Chunk {
                            stream: "stderr".into(),
                            content: format!("Error reading {stream}: {err}"),
                        });
                    }
                    break;
                }
            }
        }
    })
}

#[cfg(test)]
mod runner_tests {
    use super::{allow_interactive, working_dir};
    use std::path::PathBuf;

    #[test]
    fn interactive_gate_reads_env_flag() {
        // permitted only for a non-empty, non-"0" flag; unset/empty/"0" reject (cloud-worker default).
        assert!(allow_interactive(Some("1")));
        assert!(allow_interactive(Some("true")));
        assert!(!allow_interactive(Some("0")));
        assert!(!allow_interactive(Some("")));
        assert!(!allow_interactive(None));
    }

    #[test]
    fn working_dir_reads_env_path() {
        // a non-empty, trimmed path is used; unset/empty/blank inherit the process cwd (None).
        assert_eq!(
            working_dir(Some("/tmp/work")),
            Some(PathBuf::from("/tmp/work"))
        );
        assert_eq!(
            working_dir(Some("  /tmp/work  ")),
            Some(PathBuf::from("/tmp/work"))
        );
        assert_eq!(working_dir(Some("")), None);
        assert_eq!(working_dir(Some("   ")), None);
        assert_eq!(working_dir(None), None);
    }
}

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
            Err(err) => return Err(to_runtime_error(err)),
        }
        thread::sleep(Duration::from_millis(100));
    }
}
