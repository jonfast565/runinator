mod dynamo;

use log::info;
use runinator_models::{
    errors::{RuntimeError, SendableError},
    providers::{
        ActionMetadata, ParameterMetadata, ParameterValueType, ProviderMetadata,
        ProviderRuntimeMetadata, ResultMetadata,
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
                        ParameterMetadata::required("table_name", ParameterValueType::String),
                        ParameterMetadata::required("dump_folder", ParameterValueType::String),
                        ParameterMetadata::optional("region", ParameterValueType::String),
                        ParameterMetadata::optional("query_type", ParameterValueType::String)
                            .with_default(json!("query")),
                        ParameterMetadata::optional(
                            "key_condition_expression",
                            ParameterValueType::String,
                        ),
                        ParameterMetadata::optional(
                            "partiql_statement",
                            ParameterValueType::String,
                        ),
                        ParameterMetadata::optional("format", ParameterValueType::String)
                            .with_default(json!("excel")),
                    ])
                    .with_results(vec![
                        ResultMetadata::new("provider", ParameterValueType::String),
                        ResultMetadata::new("service", ParameterValueType::String),
                        ResultMetadata::new("rows", ParameterValueType::Integer),
                        ResultMetadata::new("artifact", ParameterValueType::Object),
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
    ) -> Result<TaskExecutionResult, SendableError> {
        info!("Running AWS provider call '{}'", request.action_function);

        match request.action_function.as_str() {
            "dynamo_dump" => {
                let result = dynamo::run_dynamo_dump(request.parameters, request.timeout_secs)?;
                Ok(TaskExecutionResult {
                    message: Some(format!(
                        "Exported {} DynamoDB row(s) to {}",
                        result.rows, result.artifact.uri
                    )),
                    output_json: Some(json!({
                        "provider": "AWS",
                        "service": "DynamoDB",
                        "rows": result.rows,
                        "artifact": result.artifact,
                    })),
                    chunks: Vec::new(),
                    artifacts: vec![result.artifact],
                })
            }
            _ => Err(Box::new(RuntimeError::new(
                "UNSUPPORTED_CALL".to_string(),
                format!(
                    "Unsupported AWS provider call '{}'",
                    request.action_function
                ),
            ))),
        }
    }
}
