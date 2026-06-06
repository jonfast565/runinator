use runinator_models::{errors::SendableError, runs::ProviderExecutionRequest};
use serde::{Deserialize, Serialize};

use crate::errors::{INVALID_PARAMS, IO};

#[derive(Deserialize)]
pub(crate) struct ConsoleParams {
    pub command: String,
}

#[derive(Serialize)]
pub(crate) struct ConsoleResult {
    pub success: bool,
    pub exit_code: i32,
    pub duration_ms: i64,
    pub command: String,
}

pub(crate) fn parse_params(
    request: &ProviderExecutionRequest,
) -> Result<ConsoleParams, SendableError> {
    serde_json::from_value(request.parameters.clone().into()).map_err(|e| INVALID_PARAMS.error(e))
}

pub(crate) fn to_runtime_error(err: std::io::Error) -> SendableError {
    IO.error(err)
}
