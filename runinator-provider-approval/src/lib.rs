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
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Deserialize)]
struct ApprovalParams {
    approval_type: Option<String>,
    prompt: Option<String>,
    #[serde(flatten)]
    metadata: serde_json::Map<String, Value>,
}

#[derive(Serialize)]
struct ApprovalResult {
    approval_type: String,
    prompt: String,
    metadata: Value,
}

#[derive(Clone)]
pub struct ApprovalProvider;

impl Provider for ApprovalProvider {
    fn name(&self) -> String {
        "approval".into()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            actions: vec![
                ActionMetadata::new("request", "Request manual approval to proceed")
                    .with_parameters(vec![
                        ParameterMetadata::optional("approval_type", ParameterValueType::String)
                            .with_default(json!("generic")),
                        ParameterMetadata::optional("prompt", ParameterValueType::String)
                            .with_default(json!("Approval required")),
                    ])
                    .with_results(vec![
                        ResultMetadata::new("approval_type", ParameterValueType::String),
                        ResultMetadata::new("prompt", ParameterValueType::String),
                        ResultMetadata::new("metadata", ParameterValueType::Object),
                    ]),
            ],
            metadata: ProviderRuntimeMetadata::default(),
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        let params = parse_params(&request)?;
        let result = ApprovalResult {
            approval_type: params.approval_type.unwrap_or_else(|| "generic".into()),
            prompt: params.prompt.unwrap_or_else(|| "Approval required".into()),
            metadata: Value::Object(params.metadata),
        };
        Ok(TaskExecutionResult {
            message: Some("Approval request prepared".into()),
            output_json: serde_json::to_value(result).ok(),
            chunks: Vec::new(),
            artifacts: Vec::new(),
        })
    }
}

fn parse_params(request: &ProviderExecutionRequest) -> Result<ApprovalParams, SendableError> {
    serde_json::from_value(request.parameters.clone()).map_err(|e| {
        Box::new(RuntimeError::new("approval.invalid_params".into(), e.to_string())) as SendableError
    })
}

#[cfg(test)]
mod tests;
