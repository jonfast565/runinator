use runinator_models::{errors::SendableError, runs::ProviderExecutionRequest};
use serde::{Deserialize, Serialize};

use crate::errors::{INVALID_PARAMS, IO};

#[derive(Deserialize)]
pub(crate) struct ConsoleParams {
    pub command: String,
    // run attached to the worker's own stdio so the command can prompt in the operator's
    // interactive desktop session (browser-based logins, macOS Keychain dialogs, tty prompts).
    // output is not captured or streamed in this mode. defaults to false: capture and stream.
    #[serde(default)]
    pub interactive: bool,
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
    runinator_provider_support::parse_params(request, &INVALID_PARAMS)
}

pub(crate) fn to_runtime_error(err: std::io::Error) -> SendableError {
    IO.error(err)
}
