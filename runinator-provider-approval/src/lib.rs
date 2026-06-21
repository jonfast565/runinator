mod errors;

use std::sync::Arc;

use runinator_models::json;
use runinator_models::value::{Map, Value};
use runinator_models::{
    errors::SendableError,
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata,
        ResultMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct ApprovalParams {
    approval_type: Option<String>,
    prompt: Option<String>,
    #[serde(flatten)]
    metadata: Map,
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
                        ParameterMetadata::optional("approval_type", RuninatorType::String)
                            .with_default(json!("generic")),
                        ParameterMetadata::optional("prompt", RuninatorType::String)
                            .with_default(json!("Approval required")),
                    ])
                    .with_results(vec![
                        ResultMetadata::new("approval_type", RuninatorType::String),
                        ResultMetadata::new("prompt", RuninatorType::String),
                        ResultMetadata::new("metadata", RuninatorType::map(RuninatorType::Any)),
                    ]),
            ],
            metadata: ProviderRuntimeMetadata::default(),
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
        _token: runinator_plugin::cancel::CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        let params = parse_params(&request)?;
        let result = ApprovalResult {
            approval_type: params.approval_type.unwrap_or_else(|| "generic".into()),
            prompt: params.prompt.unwrap_or_else(|| "Approval required".into()),
            metadata: Value::Object(params.metadata),
        };
        Ok(TaskExecutionResult {
            message: Some("Approval request prepared".into()),
            output_json: serde_json::to_value(result).ok().map(Into::into),
            chunks: Vec::new(),
            artifacts: Vec::new(),
        })
    }
}

fn parse_params(request: &ProviderExecutionRequest) -> Result<ApprovalParams, SendableError> {
    runinator_provider_support::parse_params(request, &errors::INVALID_PARAMS)
}

#[cfg(test)]
mod tests;
