use runinator_models::{
    errors::{RuntimeError, SendableError},
    runs::ProviderExecutionRequest,
};
use serde::{Deserialize, Serialize};

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
    serde_json::from_value(request.parameters.clone().into()).map_err(|e| {
        Box::new(RuntimeError::new(
            "console.invalid_params".into(),
            e.to_string(),
        )) as SendableError
    })
}

pub(crate) fn to_runtime_error(err: std::io::Error) -> SendableError {
    Box::new(RuntimeError::new("console.io".into(), err.to_string()))
}
