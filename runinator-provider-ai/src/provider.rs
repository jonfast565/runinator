use std::sync::Arc;

use runinator_models::{
    errors::SendableError,
    providers::{
        ActionMetadata, ParameterMetadata, ParameterValueType, ProviderMetadata,
        ProviderRuntimeMetadata, ResultMetadata,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde_json::json;

use crate::claude_code::run_claude_code;
use crate::params::{default_binary, default_model, default_output_format};
use crate::shell::run_shell_command;

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
                ActionMetadata::new(
                    "claude_code",
                    "Invoke Claude Code non-interactively with a prompt and model",
                )
                .with_parameters(vec![
                    ParameterMetadata::required("prompt", ParameterValueType::String),
                    ParameterMetadata::optional("model", ParameterValueType::String)
                        .with_default(json!(default_model())),
                    ParameterMetadata::optional("binary", ParameterValueType::String)
                        .with_default(json!(default_binary())),
                    ParameterMetadata::optional("working_dir", ParameterValueType::String),
                    ParameterMetadata::optional("allowed_tools", ParameterValueType::String),
                    ParameterMetadata::optional("output_format", ParameterValueType::String)
                        .with_default(json!(default_output_format())),
                    ParameterMetadata::optional("permission_mode", ParameterValueType::String),
                    ParameterMetadata::optional("extra_args", ParameterValueType::Json),
                    ParameterMetadata::optional("env", ParameterValueType::Json),
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
        sink: Option<Arc<dyn ProviderEventSink>>,
        token: runinator_plugin::cancel::CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        match request.action_function.as_str() {
            "claude_code" => run_claude_code(&request, sink, token),
            // legacy default: shell-command execution.
            _ => run_shell_command(&request, token),
        }
    }
}
