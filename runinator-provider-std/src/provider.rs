use std::sync::Arc;

use runinator_models::{
    errors::SendableError,
    providers::{ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata},
    runs::{ProviderExecutionRequest, TaskExecutionResult},
    types::RuninatorType,
    value::Value,
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use runinator_workflows::{
    ComputeOutcome, PureIntrinsics, WorkflowValidationError, effectful_signatures, parse_program,
    run_program,
};

use crate::errors::{GOTO_NOT_ALLOWED, HTTP_ERROR, INTRINSIC_FAILED, INVALID_PROGRAM};
use crate::intrinsics::FullIntrinsics;

// map an interpreter error to a SendableError, routing http failures to a dedicated code.
fn map_run_error(err: WorkflowValidationError) -> SendableError {
    match &err {
        WorkflowValidationError::IntrinsicError { name, .. } if name.starts_with("http") => {
            HTTP_ERROR.error(err.to_string())
        }
        _ => INTRINSIC_FAILED.error(err.to_string()),
    }
}

const PROGRAM_KEY: &str = "program";
const CONTEXT_KEY: &str = "context";

#[derive(Clone)]
pub struct StdProvider;

impl Provider for StdProvider {
    fn name(&self) -> String {
        "std".to_string()
    }

    fn metadata(&self) -> ProviderMetadata {
        // the two program entry points plus the library functions; pure signatures come straight
        // from PureIntrinsics so the worker's view cannot drift from the reducer's.
        let mut actions = vec![
            ActionMetadata::new("run", "evaluate a pure compute program in the reducer")
                .with_parameters(vec![ParameterMetadata::required(
                    PROGRAM_KEY,
                    RuninatorType::Any,
                )])
                .pure(),
            ActionMetadata::new("exec", "execute an effectful compute program on the worker")
                .with_parameters(vec![
                    ParameterMetadata::required(PROGRAM_KEY, RuninatorType::Any),
                    // the web service ships the run context alongside the program so the worker
                    // interpreter can resolve refs/calls against it.
                    ParameterMetadata::optional(CONTEXT_KEY, RuninatorType::Any),
                ]),
        ];
        actions.extend(PureIntrinsics::signatures());
        actions.extend(effectful_signatures());
        ProviderMetadata {
            name: self.name(),
            actions,
            metadata: ProviderRuntimeMetadata::default(),
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
        token: runinator_plugin::cancel::CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        let program_value = request
            .parameters
            .get(PROGRAM_KEY)
            .ok_or_else(|| INVALID_PROGRAM.error("missing program"))?;
        let context = request
            .parameters
            .get(CONTEXT_KEY)
            .cloned()
            .unwrap_or(Value::Null);
        let program =
            parse_program(program_value).map_err(|err| INVALID_PROGRAM.error(err.to_string()))?;
        let library = FullIntrinsics::new(request.timeout_secs, token);
        let outcome = run_program(&program, &context, &library).map_err(map_run_error)?;
        match outcome {
            ComputeOutcome::Return(value) | ComputeOutcome::Fallthrough(value) => {
                Ok(TaskExecutionResult {
                    message: None,
                    output_json: Some(value),
                    chunks: Vec::new(),
                    artifacts: Vec::new(),
                })
            }
            ComputeOutcome::Goto(target) => Err(GOTO_NOT_ALLOWED.error(target)),
        }
    }
}
