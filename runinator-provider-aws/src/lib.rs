mod dynamo;

use log::info;
use runinator_models::{
    errors::{RuntimeError, SendableError},
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

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        info!(
            "Running call '{}' w/ args `{}`",
            request.action_function, request.action_configuration
        );

        match request.action_function.as_str() {
            "dynamo_dump" => {
                let result =
                    dynamo::run_dynamo_dump(&request.action_configuration, request.timeout_secs)?;
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
