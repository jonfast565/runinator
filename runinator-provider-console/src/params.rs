use runinator_models::{errors::SendableError, runs::ProviderExecutionRequest};
use serde::{Deserialize, Serialize};

use crate::errors::{INVALID_PARAMS, IO};

pub(crate) fn parse_params(
    request: &ProviderExecutionRequest,
) -> Result<ConsoleParams, SendableError> {
    runinator_provider_support::parse_params(request, &INVALID_PARAMS)
}

pub(crate) fn parse_input_params(
    request: &ProviderExecutionRequest,
) -> Result<InputParams, SendableError> {
    runinator_provider_support::parse_params(request, &INVALID_PARAMS)
}

pub(crate) fn to_runtime_error(err: std::io::Error) -> SendableError {
    IO.error(err)
}

mod console_params;
pub(crate) use console_params::ConsoleParams;

mod input_params;
pub(crate) use input_params::InputParams;

mod console_result;
pub(crate) use console_result::ConsoleResult;

mod input_result;
pub(crate) use input_result::InputResult;
