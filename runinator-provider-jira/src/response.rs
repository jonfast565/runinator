use runinator_models::{errors::SendableError, runs::TaskExecutionResult};

pub(crate) fn json_response(
    output: serde_json::Value,
) -> Result<TaskExecutionResult, SendableError> {
    Ok(TaskExecutionResult {
        message: Some("jira action completed".into()),
        output_json: Some(output.into()),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}
