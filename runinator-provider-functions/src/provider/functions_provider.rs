#[allow(unused_imports)]
use super::*;

pub struct FunctionsProvider {
    pub(super) runtime: Arc<dyn InvocationRuntime>,
}

impl Default for FunctionsProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FunctionsProvider {
    pub fn new() -> Self {
        Self {
            runtime: Arc::new(DockerInvocationRuntime::new()),
        }
    }

    /// build one over a specific runtime, which is how a kubernetes-job backend will slot in.
    pub fn with_runtime(runtime: Arc<dyn InvocationRuntime>) -> Self {
        Self { runtime }
    }
}

impl Provider for FunctionsProvider {
    fn name(&self) -> String {
        FUNCTIONS_PROVIDER.to_string()
    }

    fn metadata(&self) -> ProviderMetadata {
        // exactly one action. the worker checks an action's function against this metadata before
        // executing, so per-export names would be rejected — and no static list could enumerate
        // every export ever published anyway. the export is named by the action's `FunctionBinding`.
        //
        // the shape comes from `runinator-models` because the engine writes the same metadata into
        // the catalog on publish: publishing must not wait for a worker to register the provider.
        runinator_models::functions::functions_provider_metadata()
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        sink: Option<Arc<dyn ProviderEventSink>>,
        token: CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        if request.action_function != FUNCTIONS_INVOKE {
            return Err(INVALID_REQUEST.error(format!(
                "unknown function '{}'; the functions provider offers only '{FUNCTIONS_INVOKE}'",
                request.action_function
            )));
        }
        let invocation = InvocationRequest::parse(&request)?;
        // logs stream while the container runs rather than being replayed after it, so a long
        // invocation is observable instead of silent until it ends.
        let logs = sink
            .clone()
            .map(|sink| Arc::new(EventSink(sink)) as Arc<dyn LineSink>);
        let outcome = self.runtime.invoke(&invocation, logs, token)?;

        let mut message = format!(
            "{} completed in {:.2}s",
            invocation.handler,
            outcome.duration.as_secs_f64()
        );
        if outcome.truncated {
            // said explicitly rather than presenting a partial log as the whole of one.
            message.push_str(" (output truncated)");
        }
        Ok(TaskExecutionResult {
            message: Some(message),
            output_json: Some(outcome.output),
            chunks: Vec::new(),
            artifacts: Vec::new(),
        })
    }
}
