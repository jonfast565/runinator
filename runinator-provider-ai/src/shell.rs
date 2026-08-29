use std::io::Write;
use std::process::{Child, ExitStatus, Stdio};
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

use crate::errors::{CANCELED, INVALID_JSON, NONZERO_EXIT, TIMEOUT};
use crate::params::{AiCommandParams, parse_params};

pub(crate) fn run_shell_command(
    request: &ProviderExecutionRequest,
    sink: Option<Arc<dyn ProviderEventSink>>,
    token: CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    let params: AiCommandParams = parse_params(request)?;
    if token.is_cancelled() {
        return Err(CANCELED.bare());
    }
    let input = params.input.unwrap_or_else(|| json!({}));
    let mut command = runinator_platform::shell::shell_command(&params.command);
    if let Some(dir) =
        runinator_provider_support::resolve_working_dir(request.workspace_path.as_deref(), None)?
    {
        command.current_dir(dir);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let output = ProcessOutputPump::start(&mut child, sink)?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(serde_json::to_string(&input)?.as_bytes())?;
    }
    let status = wait_with_timeout(&mut child, request.timeout_secs, token);
    let output = output.finish();
    let status = status?;
    if !status.success() {
        return Err(NONZERO_EXIT.error(&output.stderr));
    }
    let parsed: Value = serde_json::from_str(&output.stdout)
        .map_err(|err| INVALID_JSON.error(format!("AI command stdout must be JSON: {err}")))?;
    Ok(TaskExecutionResult {
        message: Some("AI command completed".into()),
        output_json: Some(parsed),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn wait_with_timeout(
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
            return Err(CANCELED.bare());
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(TIMEOUT.error(format!(
                "AI command timed out after {} seconds",
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
