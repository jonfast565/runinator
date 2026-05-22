use runinator_models::{
    errors::{RuntimeError, SendableError},
    runs::TaskExecutionResult,
};
use serde_json::json;

pub(crate) fn json_response(
    provider: &str,
    response: reqwest::blocking::Response,
) -> Result<TaskExecutionResult, SendableError> {
    let status = response.status();
    let text = response.text()?;
    if !status.is_success() {
        return Err(Box::new(RuntimeError::new(
            format!("{provider}.http_error"),
            format!("HTTP {status}: {text}"),
        )));
    }
    let output = if text.trim().is_empty() {
        json!({ "status": status.as_u16() })
    } else {
        serde_json::from_str(&text)
            .unwrap_or_else(|_| json!({ "body": text, "status": status.as_u16() }))
    };
    Ok(TaskExecutionResult {
        message: Some(format!("{provider} action completed")),
        output_json: Some(output),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}
