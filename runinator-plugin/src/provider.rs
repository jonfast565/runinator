use std::sync::{Arc, mpsc::Receiver};

use runinator_models::{
    errors::SendableError,
    providers::ProviderMetadata,
    runs::{
        ProviderExecutionEvent, ProviderExecutionRequest, ProviderTerminalControl,
        TaskExecutionResult,
    },
};

use crate::cancel::CancellationToken;

pub trait ProviderEventSink: Send + Sync {
    fn emit(&self, event: ProviderExecutionEvent);

    /// Transfer this effect's terminal-control receiver to a provider that owns an interactive
    /// session. The default preserves compatibility for sinks outside the worker runtime.
    fn take_terminal_control(&self) -> Option<Receiver<ProviderTerminalControl>> {
        None
    }
}

pub trait Provider: Send + Sync {
    fn name(&self) -> String;

    fn metadata(&self) -> ProviderMetadata;

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        sink: Option<Arc<dyn ProviderEventSink>>,
        token: CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError>;
}
