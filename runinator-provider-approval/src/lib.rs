use std::sync::Arc;

use runinator_models::{
    errors::SendableError,
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct ApprovalProvider;

impl Provider for ApprovalProvider {
    fn name(&self) -> String {
        "approval".into()
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        Ok(TaskExecutionResult {
            message: Some("Approval request prepared".into()),
            output_json: Some(json!({
                "approval_type": str_param(&request.parameters, "approval_type").unwrap_or("generic"),
                "prompt": str_param(&request.parameters, "prompt").unwrap_or("Approval required"),
                "metadata": request.parameters,
            })),
            chunks: Vec::new(),
            artifacts: Vec::new(),
        })
    }
}

fn str_param<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

#[cfg(test)]
mod tests;
