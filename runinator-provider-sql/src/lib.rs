mod connector;
mod dump;
mod format;
mod helpers;

use std::sync::Arc;

use runinator_models::{
    errors::{RuntimeError, SendableError},
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata,
        ResultMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde_json::json;

#[derive(Clone)]
pub struct SqlProvider;

impl Provider for SqlProvider {
    fn name(&self) -> String {
        "SQL".to_string()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            actions: vec![
                ActionMetadata::new(
                    "dump_data",
                    "Execute SQL queries and export results to Excel/CSV",
                )
                .with_parameters(vec![
                    ParameterMetadata::required("database", RuninatorType::String),
                    ParameterMetadata::required("connection_string", RuninatorType::String)
                        .secret(),
                    ParameterMetadata::required("dump_folder", RuninatorType::String),
                    ParameterMetadata::required("queries", RuninatorType::map(RuninatorType::Any)),
                    ParameterMetadata::optional("file_prefix", RuninatorType::String),
                    ParameterMetadata::optional("format", RuninatorType::String)
                        .with_default(json!("excel")),
                ])
                .with_results(vec![
                    ResultMetadata::new("provider", RuninatorType::String),
                    ResultMetadata::new("exports", RuninatorType::map(RuninatorType::Any)),
                ]),
            ],
            metadata: ProviderRuntimeMetadata {
                credential_scopes: vec!["sql".into()],
                contract: None,
            },
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
        token: runinator_plugin::cancel::CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        match request.action_function.as_str() {
            "dump_data" => self.dump_data(request.parameters, request.timeout_secs, token),
            _ => Err(Box::new(RuntimeError::new(
                "UNSUPPORTED_CALL".to_string(),
                format!(
                    "Unsupported SQL provider call '{}'",
                    request.action_function
                ),
            ))),
        }
    }
}
