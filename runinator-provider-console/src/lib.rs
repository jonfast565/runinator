use std::{
    io::{BufRead, BufReader},
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
    errors::{RuntimeError, SendableError},
    providers::{
        ActionMetadata, ParameterMetadata, ParameterValueType, ProviderMetadata,
        ProviderRuntimeMetadata, ResultMetadata,
    },
    runs::{ProviderExecutionEvent, ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct ConsoleParams {
    command: String,
}

#[derive(Serialize)]
struct ConsoleResult {
    success: bool,
    exit_code: i32,
    duration_ms: i64,
    command: String,
}

#[derive(Clone)]
pub struct ConsoleProvider;

impl Provider for ConsoleProvider {
    fn name(&self) -> String {
        "Console".to_string()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            actions: vec![
                ActionMetadata::new("run", "Run a shell command")
                    .with_parameters(vec![ParameterMetadata::required(
                        "command",
                        ParameterValueType::String,
                    )])
                    .with_results(vec![
                        ResultMetadata::new("success", ParameterValueType::Boolean),
                        ResultMetadata::new("exit_code", ParameterValueType::Integer),
                        ResultMetadata::new("duration_ms", ParameterValueType::Integer),
                        ResultMetadata::new("command", ParameterValueType::String),
                    ]),
            ],
            metadata: ProviderRuntimeMetadata::default(),
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        execute_command(&request, sink)
    }
}

fn parse_params(request: &ProviderExecutionRequest) -> Result<ConsoleParams, SendableError> {
    serde_json::from_value(request.parameters.clone()).map_err(|e| {
        Box::new(RuntimeError::new(
            "console.invalid_params".into(),
            e.to_string(),
        )) as SendableError
    })
}

fn execute_command(
    request: &ProviderExecutionRequest,
    sink: Option<Arc<dyn ProviderEventSink>>,
) -> Result<TaskExecutionResult, SendableError> {
    let params = parse_params(request)?;
    let command_text = params.command;
    let started = Instant::now();

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", &command_text]);
        cmd
    };

    #[cfg(not(target_os = "windows"))]
    let mut command = {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", &command_text]);
        cmd
    };

    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(to_runtime_error)?;
    let stdout = child.stdout.take().ok_or_else(|| {
        RuntimeError::new(
            "console.stdout.unavailable".into(),
            "Failed to capture command stdout".into(),
        )
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        RuntimeError::new(
            "console.stderr.unavailable".into(),
            "Failed to capture command stderr".into(),
        )
    })?;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stdout_thread = spawn_output_thread(stdout, Arc::clone(&stop_flag), "stdout", sink.clone());
    let stderr_thread = spawn_output_thread(stderr, Arc::clone(&stop_flag), "stderr", sink);
    let timeout = Duration::from_secs(request.timeout_secs.max(1) as u64);
    let status = wait_for_child(&mut child, timeout, started)?;

    stop_flag.store(true, Ordering::Relaxed);
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

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
            output_json: serde_json::to_value(result).ok(),
            chunks: Vec::new(),
            artifacts: Vec::new(),
        })
    } else {
        Err(Box::new(RuntimeError::new(
            "console.nonzero_exit".into(),
            format!("Console command exited with code {exit_code}"),
        )))
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

fn wait_for_child(
    child: &mut Child,
    timeout: Duration,
    start: Instant,
) -> Result<ExitStatus, SendableError> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    warn!("Console child exceeded timeout; killing process");
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Box::new(RuntimeError::new(
                        "console.timeout".into(),
                        format!(
                            "Console command timed out after {} seconds",
                            timeout.as_secs()
                        ),
                    )));
                }
            }
            Err(err) => return Err(to_runtime_error(err)),
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn to_runtime_error(err: std::io::Error) -> SendableError {
    Box::new(RuntimeError::new("console.io".into(), err.to_string()))
}
