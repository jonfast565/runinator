use std::sync::Arc;

use runinator_models::{
    errors::SendableError,
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata,
        ResultMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};

use crate::{
    errors::INVALID_PARAMS,
    runner::{execute_command, execute_input},
};

#[derive(Clone)]
pub struct ConsoleProvider;

impl Provider for ConsoleProvider {
    fn name(&self) -> String {
        "console".to_string()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            actions: vec![
                ActionMetadata::new("run", "Run a shell command")
                    .with_parameters(vec![
                        ParameterMetadata::required("command", RuninatorType::String),
                        ParameterMetadata::optional("interactive", RuninatorType::Boolean)
                            .with_description(
                                "run attached to the worker's desktop session so the command can \
                                 prompt (browser login, Keychain dialog); output is not streamed",
                            ),
                    ])
                    .with_results(vec![
                        ResultMetadata::new("success", RuninatorType::Boolean),
                        ResultMetadata::new("exit_code", RuninatorType::Integer),
                        ResultMetadata::new("duration_ms", RuninatorType::Integer),
                        ResultMetadata::new("command", RuninatorType::String),
                    ]),
                ActionMetadata::new("input", "Prompt for one line of terminal input")
                    .with_parameters(vec![ParameterMetadata::required(
                        "prompt",
                        RuninatorType::String,
                    )])
                    .with_results(vec![ResultMetadata::new("value", RuninatorType::String)]),
            ],
            metadata: ProviderRuntimeMetadata::default(),
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        sink: Option<Arc<dyn ProviderEventSink>>,
        token: runinator_plugin::cancel::CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        match request.action_function.as_str() {
            "run" => execute_command(&request, sink, token),
            "input" => execute_input(&request, sink, token),
            other => Err(INVALID_PARAMS.error(format!("unsupported console function '{other}'"))),
        }
    }
}
