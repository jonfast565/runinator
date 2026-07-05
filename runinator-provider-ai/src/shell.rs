use std::io::Write;
use std::process::{Child, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use runinator_models::json;
use runinator_models::value::Value;
use runinator_models::{
    errors::SendableError,
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::cancel::CancellationToken;

use crate::errors::{CANCELED, INVALID_JSON, NONZERO_EXIT, TIMEOUT};
use crate::params::{AiCommandParams, parse_params};

pub(crate) fn run_shell_command(
    request: &ProviderExecutionRequest,
    token: CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    let params: AiCommandParams = parse_params(request)?;
    if token.is_cancelled() {
        return Err(CANCELED.bare());
    }
    let input = params.input.unwrap_or_else(|| json!({}));
    let mut child = runinator_utilities::shell::shell_command(&params.command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(serde_json::to_string(&input)?.as_bytes())?;
    }
    if token.is_cancelled() {
        let _ = child.kill();
        return Err(CANCELED.bare());
    }
    let output = wait_with_timeout(child, request.timeout_secs, token)?;
    if !output.status.success() {
        return Err(NONZERO_EXIT.error(String::from_utf8_lossy(&output.stderr)));
    }
    let stdout = String::from_utf8(output.stdout)?;
    let parsed: Value = serde_json::from_str(&stdout)
        .map_err(|err| INVALID_JSON.error(format!("AI command stdout must be JSON: {err}")))?;
    Ok(TaskExecutionResult {
        message: Some("AI command completed".into()),
        output_json: Some(parsed),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn wait_with_timeout(
    mut child: Child,
    timeout_secs: i64,
    token: CancellationToken,
) -> Result<std::process::Output, SendableError> {
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
        if child.try_wait()?.is_some() {
            return child
                .wait_with_output()
                .map_err(|err| -> SendableError { Box::new(err) });
        }
        thread::sleep(Duration::from_millis(100));
    }
}
