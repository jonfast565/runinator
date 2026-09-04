//! Workspace completion checkpoints; the worker owns snapshots and uploads.
pub mod errors {
    pub use runinator_models::errors::{WORKSPACE_DICTIONARY as DICTIONARY, WORKSPACE_INVALID};
}
use runinator_models::{
    errors::SendableError,
    providers::{ActionMetadata, ParameterMetadata, ProviderMetadata},
    runs::{ProviderExecutionRequest, TaskExecutionResult},
    types::RuninatorType,
};
use runinator_plugin::{
    cancel::CancellationToken,
    provider::{Provider, ProviderEventSink},
};
use std::sync::Arc;

pub struct WorkspaceProvider;
impl Provider for WorkspaceProvider {
    fn name(&self) -> String {
        "workspace".into()
    }
    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            metadata: Default::default(),
            actions: vec![
                ActionMetadata::new(
                    "checkpoint",
                    "save the attached workspace and structured result",
                )
                .with_parameters(vec![ParameterMetadata::optional(
                    "value",
                    RuninatorType::Any,
                )]),
            ],
        }
    }
    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
        token: CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        if request.action_function != "checkpoint"
            || request.workspace_path.is_none()
            || token.is_cancelled()
        {
            return Err(errors::WORKSPACE_INVALID.error("checkpoint requires an active workspace"));
        }
        Ok(TaskExecutionResult {
            message: None,
            output_json: request.parameters.get("value").cloned(),
            chunks: vec![],
            artifacts: vec![],
        })
    }
}
