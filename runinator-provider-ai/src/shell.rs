use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use runinator_models::json;
use runinator_models::value::Value;
use runinator_models::{
    errors::{RuntimeError, SendableError},
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::cancel::CancellationToken;

use crate::params::{AiCommandParams, parse_params};

pub(crate) fn run_shell_command(
    request: &ProviderExecutionRequest,
    token: CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    let params: AiCommandParams = parse_params(request)?;
    if token.is_cancelled() {
        return Err(Box::new(RuntimeError::new(
            "ai_command.canceled".into(),
            "AI command canceled".into(),
        )));
    }
    let input = params.input.unwrap_or_else(|| json!({}));
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&params.command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(serde_json::to_string(&input)?.as_bytes())?;
    }
    if token.is_cancelled() {
        let _ = child.kill();
        return Err(Box::new(RuntimeError::new(
            "ai_command.canceled".into(),
            "AI command canceled".into(),
        )));
    }
    let output = wait_with_timeout(child, request.timeout_secs, token)?;
    if !output.status.success() {
        return Err(Box::new(RuntimeError::new(
            "ai_command.nonzero_exit".into(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        )));
    }
    let stdout = String::from_utf8(output.stdout)?;
    let parsed: Value = serde_json::from_str(&stdout).map_err(|err| {
        RuntimeError::new(
            "ai_command.invalid_json".into(),
            format!("AI command stdout must be JSON: {err}"),
        )
    })?;
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
            return Err(Box::new(RuntimeError::new(
                "ai_command.canceled".into(),
                "AI command canceled".into(),
            )));
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Box::new(RuntimeError::new(
                "ai_command.timeout".into(),
                format!("AI command timed out after {} seconds", timeout.as_secs()),
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
