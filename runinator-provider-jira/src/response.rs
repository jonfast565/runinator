use runinator_models::json;
use runinator_models::{errors::SendableError, runs::TaskExecutionResult};

use crate::error::HTTP_ERROR;

// builds the standard http-error for a non-success status and body snippet.
pub(crate) fn http_status_error(status: reqwest::StatusCode, body: &str) -> SendableError {
    HTTP_ERROR.error(format!("HTTP {status}: {body}"))
}

pub(crate) fn json_response(
    response: reqwest::blocking::Response,
) -> Result<TaskExecutionResult, SendableError> {
    let status = response.status();
    let text = response.text()?;
    if !status.is_success() {
        return Err(HTTP_ERROR.error(format!("HTTP {status}: {text}")));
    }
    let output = if text.trim().is_empty() {
        json!({ "status": status.as_u16() })
    } else {
        serde_json::from_str(&text)
            .unwrap_or_else(|_| json!({ "body": text, "status": status.as_u16() }))
    };
    Ok(TaskExecutionResult {
        message: Some("jira action completed".into()),
        output_json: Some(output),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}
