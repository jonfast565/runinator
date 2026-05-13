use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Arc;

use runinator_models::{
    errors::{RuntimeError, SendableError},
    providers::{
        ActionMetadata, ParameterMetadata, ParameterValueType, ProviderMetadata,
        ProviderRuntimeMetadata, ResultMetadata,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct AiCommandParams {
    command: String,
    input: Option<Value>,
}

#[derive(Clone)]
pub struct AiCommandProvider;

impl Provider for AiCommandProvider {
    fn name(&self) -> String {
        "ai-command".into()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            actions: vec![
                ActionMetadata::new("execute", "Run an AI command via shell")
                    .with_parameters(vec![
                        ParameterMetadata::required("command", ParameterValueType::String),
                        ParameterMetadata::optional("input", ParameterValueType::Json),
                    ])
                    .with_results(vec![ResultMetadata::new(
                        "response",
                        ParameterValueType::Json,
                    )]),
            ],
            metadata: ProviderRuntimeMetadata {
                credential_scopes: Vec::new(),
                contract: Some("stdin/stdout JSON".into()),
            },
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        let params = parse_params(&request)?;
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
}

fn parse_params(request: &ProviderExecutionRequest) -> Result<AiCommandParams, SendableError> {
    serde_json::from_value(request.parameters.clone()).map_err(|e| {
        Box::new(RuntimeError::new(
            "ai_command.invalid_params".into(),
            e.to_string(),
        )) as SendableError
    })
}

#[cfg(test)]
mod tests;
