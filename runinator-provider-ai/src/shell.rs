use std::io::Write;
use std::process::{Command, Stdio};

use runinator_models::{
    errors::{RuntimeError, SendableError},
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use serde_json::{Value, json};

use crate::params::{AiCommandParams, parse_params};

pub(crate) fn run_shell_command(
    request: &ProviderExecutionRequest,
) -> Result<TaskExecutionResult, SendableError> {
    let params: AiCommandParams = parse_params(request)?;
    let input = params.input.unwrap_or_else(|| json!({}));
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&params.command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(serde_json::to_string(&input)?.as_bytes())?;
    }
    let output = child.wait_with_output()?;
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
