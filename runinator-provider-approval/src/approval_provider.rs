#[allow(unused_imports)]
use super::*;

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
