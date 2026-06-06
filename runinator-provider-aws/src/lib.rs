mod dynamo;
mod errors;

use log::info;
use runinator_models::{
    errors::SendableError,
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata,
        ResultMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct AwsProvider;

impl Provider for AwsProvider {
    fn name(&self) -> String {
        "AWS".to_string()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            actions: vec![
                ActionMetadata::new("dynamo_dump", "Export DynamoDB rows to an artifact")
                    .with_parameters(vec![
                        ParameterMetadata::required("table_name", RuninatorType::String),
                        ParameterMetadata::required("dump_folder", RuninatorType::String),
                        ParameterMetadata::optional("region", RuninatorType::String),
                        ParameterMetadata::optional("query_type", RuninatorType::String)
                            .with_default(json!("query")),
                        ParameterMetadata::optional(
                            "key_condition_expression",
                            RuninatorType::String,
                        ),
                        ParameterMetadata::optional("partiql_statement", RuninatorType::String),
                        ParameterMetadata::optional("format", RuninatorType::String)
                            .with_default(json!("excel")),
                    ])
                    .with_results(vec![
                        ResultMetadata::new("provider", RuninatorType::String),
                        ResultMetadata::new("service", RuninatorType::String),
                        ResultMetadata::new("rows", RuninatorType::Integer),
                        ResultMetadata::new("artifact", artifact_type()),
                    ]),
            ],
            metadata: ProviderRuntimeMetadata {
                credential_scopes: vec!["aws".into()],
                contract: None,
            },
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
        _token: runinator_plugin::cancel::CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        info!("Running AWS provider call '{}'", request.action_function);

        match request.action_function.as_str() {
            "dynamo_dump" => {
                let result =
                    dynamo::run_dynamo_dump(request.parameters.into(), request.timeout_secs)?;
                Ok(TaskExecutionResult {
                    message: Some(format!(
                        "Exported {} DynamoDB row(s) to {}",
                        result.rows, result.artifact.uri
                    )),
                    output_json: Some(
                        json!({
                            "provider": "AWS",
                            "service": "DynamoDB",
                            "rows": result.rows,
                            "artifact": result.artifact,
                        })
                        .into(),
                    ),
                    chunks: Vec::new(),
                    artifacts: vec![result.artifact],
                })
            }
            _ => Err(errors::UNSUPPORTED_CALL.error(format!(
                "Unsupported AWS provider call '{}'",
                request.action_function
            ))),
        }
    }
}

fn artifact_type() -> RuninatorType {
    RuninatorType::structure([
        ("name", RuninatorType::String),
        ("mime_type", RuninatorType::String),
        ("size_bytes", RuninatorType::Integer),
        ("uri", RuninatorType::String),
        ("metadata", RuninatorType::map(RuninatorType::Any)),
    ])
}
