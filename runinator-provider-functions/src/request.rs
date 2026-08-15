//! the parameters a worker injects for one invocation.
//!
//! these are not authored: a workflow writes `functions.image_tools.resize(source: ...)`, and the
//! worker turns the action's `FunctionBinding` into a staged package plus these keys. keeping them
//! in one place is what stops the worker's staging and the provider's reading from drifting.

use std::path::PathBuf;

use runinator_models::errors::SendableError;
use runinator_models::functions::{FunctionResourceLimits, FunctionRuntimeSpec};
use runinator_models::runs::ProviderExecutionRequest;
use runinator_models::value::Value;

use crate::errors::INVALID_REQUEST;

// the key names are defined in `runinator-models` because the engine also writes them into the
// provider catalog on publish; re-exported here so this module still reads as their home.
pub use runinator_models::functions::{
    INVOKE_CONTEXT as CONTEXT_KEY, INVOKE_HANDLER as HANDLER_KEY, INVOKE_INPUT as INPUT_KEY,
    INVOKE_LIMITS as LIMITS_KEY, INVOKE_PACKAGE_PATH as PACKAGE_PATH_KEY,
    INVOKE_RUNTIME as RUNTIME_KEY,
};

/// one invocation, as the provider reads it.
#[derive(Debug, Clone)]
pub struct InvocationRequest {
    pub package_path: PathBuf,
    pub handler: String,
    pub runtime: FunctionRuntimeSpec,
    pub limits: FunctionResourceLimits,
    pub input: Value,
    pub context: Value,
    /// the node's own timeout, which caps the export's declared one.
    pub timeout_secs: i64,
}

impl InvocationRequest {
    pub fn parse(request: &ProviderExecutionRequest) -> Result<Self, SendableError> {
        let package_path = string_param(request, PACKAGE_PATH_KEY)?;
        let package_path = PathBuf::from(package_path);
        if !package_path.is_dir() {
            return Err(INVALID_REQUEST.error(format!(
                "package path '{}' is not a staged directory",
                package_path.display()
            )));
        }
        let handler = string_param(request, HANDLER_KEY)?;
        let runtime: FunctionRuntimeSpec = decode(request, RUNTIME_KEY)?;
        // limits default rather than fail: an omitted limit means "the default", never "unlimited",
        // and a version published before a limit existed must still run bounded.
        let limits: FunctionResourceLimits = request
            .parameters
            .get(LIMITS_KEY)
            .map(|value| value.decode::<FunctionResourceLimits>())
            .transpose()
            .map_err(|err| INVALID_REQUEST.error(format!("invalid {LIMITS_KEY}: {err}")))?
            .unwrap_or_default();

        Ok(Self {
            package_path,
            handler,
            runtime,
            limits,
            input: request
                .parameters
                .get(INPUT_KEY)
                .cloned()
                .unwrap_or(Value::Null),
            context: request
                .parameters
                .get(CONTEXT_KEY)
                .cloned()
                .unwrap_or(Value::Null),
            timeout_secs: request.timeout_secs,
        })
    }

    /// the deadline this invocation actually runs under.
    ///
    /// the smaller of the export's declared limit and the node's timeout: the manifest cannot buy
    /// itself more time than the workflow allowed, and the workflow cannot make an export run longer
    /// than its author said it should.
    pub fn effective_timeout_secs(&self) -> u64 {
        let declared = self.limits.timeout_seconds.max(1) as u64;
        if self.timeout_secs <= 0 {
            return declared;
        }
        declared.min(self.timeout_secs as u64)
    }
}

fn string_param(request: &ProviderExecutionRequest, name: &str) -> Result<String, SendableError> {
    request
        .parameters
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| INVALID_REQUEST.error(format!("missing string parameter '{name}'")))
}

fn decode<T: serde::de::DeserializeOwned>(
    request: &ProviderExecutionRequest,
    name: &str,
) -> Result<T, SendableError> {
    request
        .parameters
        .get(name)
        .ok_or_else(|| INVALID_REQUEST.error(format!("missing parameter '{name}'")))?
        .decode::<T>()
        .map_err(|err| INVALID_REQUEST.error(format!("invalid {name}: {err}")))
}
