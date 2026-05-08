use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Arc;

use runinator_models::{
    errors::{RuntimeError, SendableError},
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct AiCommandProvider;

impl Provider for AiCommandProvider {
    fn name(&self) -> String {
        "ai-command".into()
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        let command = required(&request.parameters, "command")?;
        let input = request
            .parameters
            .get("input")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(command)
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
}

fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, SendableError> {
    str_param(value, key).ok_or_else(|| {
        Box::new(RuntimeError::new(
            "provider.missing_parameter".into(),
            format!("Missing required parameter '{key}'"),
        )) as SendableError
    })
}

fn str_param<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

#[cfg(test)]
mod tests;
