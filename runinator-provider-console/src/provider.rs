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

use crate::runner::execute_command;

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
                        RuninatorType::String,
                    )])
                    .with_results(vec![
                        ResultMetadata::new("success", RuninatorType::Boolean),
                        ResultMetadata::new("exit_code", RuninatorType::Integer),
                        ResultMetadata::new("duration_ms", RuninatorType::Integer),
                        ResultMetadata::new("command", RuninatorType::String),
                    ]),
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
        execute_command(&request, sink, token)
    }
}
