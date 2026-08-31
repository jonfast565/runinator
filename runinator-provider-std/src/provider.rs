use std::sync::Arc;

use runinator_compute::{PureIntrinsics, WorkflowValidationError, effectful_signatures};
use runinator_models::{
    errors::SendableError,
    foreign_languages::ForeignLanguage,
    providers::{ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata},
    runs::{ProviderExecutionRequest, TaskExecutionResult},
    types::{RuninatorField, RuninatorType},
    value::Value,
};
use runinator_plugin::provider::{Provider, ProviderEventSink};

use crate::code::{EXPECTED_OUTPUT_TYPE_KEY, execute_code};
use crate::errors::{HTTP_ERROR, INTRINSIC_FAILED};
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

const LANGUAGE_KEY: &str = "language";
const SOURCE_KEY: &str = "source";
const RUNTIME_KEY: &str = "runtime";
const CONTEXT_KEY: &str = "context";
const CODE_FUNCTION: &str = "code";

#[derive(Clone)]
pub struct StdProvider;

impl Provider for StdProvider {
    fn name(&self) -> String {
        "std".to_string()
    }

    fn metadata(&self) -> ProviderMetadata {
        let mut actions = vec![
            ActionMetadata::new("code", "execute foreign compute code in a docker container")
                .with_parameters(vec![
                    ParameterMetadata::required(LANGUAGE_KEY, foreign_language_type())
                        .with_description("Language used to compile or run the source code."),
                    ParameterMetadata::required(SOURCE_KEY, RuninatorType::String)
                        .with_description("Source code executed inside the configured container."),
                    ParameterMetadata::required(RUNTIME_KEY, code_runtime_type()).with_description(
                        "Container image, optional toolchain overrides, environment, and limits.",
                    ),
                    ParameterMetadata::optional(CONTEXT_KEY, RuninatorType::Any).with_description(
                        "JSON value exposed to the foreign program as its input context.",
                    ),
                    ParameterMetadata::optional(EXPECTED_OUTPUT_TYPE_KEY, RuninatorType::Any)
                        .with_description(
                            "Optional Runinator type used to validate the returned JSON value.",
                        ),
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
        if request.action_function == CODE_FUNCTION {
            return execute_code(&request, _sink, token);
        }
        // a single named intrinsic, dispatched by an `invocation` node's yielded call.
        //
        // this is the path the invocation runtime uses, and it is why the provider advertises every
        // intrinsic as an action rather than only `run`/`exec`. the reducer has already decided
        // which one function it wants, so shipping a whole program plus the run context — which the
        // `run`/`exec` path below still does — would send everything the worker no longer needs in
        // order to decide anything.
        let declared = self
            .metadata()
            .actions
            .iter()
            .find(|action| action.function_name == request.action_function)
            .map(|action| {
                action
                    .parameters
                    .iter()
                    .map(|parameter| parameter.name.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        execute_intrinsic(&request, &declared, token)
    }
}

fn foreign_language_type() -> RuninatorType {
    RuninatorType::Enum(
        ForeignLanguage::ALL
            .iter()
            .map(|language| Value::from(language.canonical()))
            .collect(),
    )
}

fn positive_integer() -> RuninatorType {
    RuninatorType::Range {
        base: Box::new(RuninatorType::Integer),
        min: Some(Value::from(1)),
        max: None,
    }
}

fn code_runtime_type() -> RuninatorType {
    RuninatorType::typed_structure([
        ("image", RuninatorField::required(RuninatorType::String)),
        (
            "setup_script",
            RuninatorField::optional(RuninatorType::String),
        ),
        (
            "environment",
            RuninatorField::optional(RuninatorType::map(RuninatorType::String)),
        ),
        (
            "toolchain",
            RuninatorField::optional(RuninatorType::typed_structure([
                (
                    "executable",
                    RuninatorField::optional(RuninatorType::String),
                ),
                (
                    "build_args",
                    RuninatorField::optional(RuninatorType::array(RuninatorType::String)),
                ),
                (
                    "run_args",
                    RuninatorField::optional(RuninatorType::array(RuninatorType::String)),
                ),
            ])),
        ),
        (
            "limits",
            RuninatorField::optional(RuninatorType::typed_structure([
                ("memory_mb", RuninatorField::optional(positive_integer())),
                ("cpu_millis", RuninatorField::optional(positive_integer())),
                ("pids", RuninatorField::optional(positive_integer())),
                ("tmpfs_mb", RuninatorField::optional(positive_integer())),
                (
                    "max_output_bytes",
                    RuninatorField::optional(positive_integer()),
                ),
            ])),
        ),
    ])
}

/// run one named intrinsic against the arguments an invocation call carried.
///
/// arguments arrive under the parameter names this action's metadata declares, because the worker
/// validates them against that metadata as a closed struct before the provider is reached — an
/// undeclared key fails the action rather than arriving here. `declared` is that name list in
/// declaration order, which is what turns the named object back into the positional list the
/// library takes.
fn execute_intrinsic(
    request: &ProviderExecutionRequest,
    declared: &[String],
    token: runinator_plugin::cancel::CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    let mut args = Vec::new();
    for name in declared {
        // stop at the first absent parameter rather than filling it with null: a trailing optional
        // the caller omitted means the library should see a shorter argument list, not a longer one
        // ending in null, which several intrinsics treat as a real value.
        let Some(value) = request.parameters.get(name.as_str()) else {
            break;
        };
        args.push(value.clone());
    }
    let library = FullIntrinsics::new(request.timeout_secs, token);
    let value =
        runinator_compute::IntrinsicLibrary::call(&library, &request.action_function, &args)
            .map_err(map_run_error)?;
    Ok(TaskExecutionResult {
        message: None,
        output_json: Some(value),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}
