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
