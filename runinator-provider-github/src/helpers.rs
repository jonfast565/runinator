use runinator_models::{
    errors::SendableError,
    providers::{ParameterMetadata, ResultMetadata, RuninatorType},
    runs::TaskExecutionResult,
};
use serde_json::{Value, json};

use crate::errors::REVISION_MISMATCH;

runinator_provider_support::provider_parse_params!(crate::errors::INVALID_PARAMS);

pub(crate) fn first_pull_number(value: &Value) -> Option<i64> {
    value
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item.get("number"))
        .and_then(Value::as_i64)
}

pub(crate) fn checks_summary_response(raw: Value) -> Result<TaskExecutionResult, SendableError> {
    let summary = summarize_check_runs(raw);
    Ok(TaskExecutionResult {
        message: Some("github checks summary completed".into()),
        output_json: Some(summary.into()),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

pub(crate) fn exact_revision_checks_summary_response(
    raw: Value,
    expected_revision: &str,
) -> Result<TaskExecutionResult, SendableError> {
    let mismatched = raw
        .get("check_runs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|run| run.get("head_sha").and_then(Value::as_str))
        .find(|head_sha| *head_sha != expected_revision);
    if let Some(actual) = mismatched {
        return Err(
            REVISION_MISMATCH.error(format!("expected {expected_revision}, received {actual}"))
        );
    }
    let mut summary = summarize_check_runs(raw);
    if let Value::Object(fields) = &mut summary {
        fields.insert("revision".into(), json!(expected_revision));
    }
    Ok(TaskExecutionResult {
        message: Some(format!(
            "github checks summary completed for {expected_revision}"
        )),
        output_json: Some(summary.into()),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

pub(crate) fn summarize_check_runs(raw: Value) -> Value {
    let mut passed = 0;
    let mut pending = 0;
    let mut failed = 0;
    let check_runs = raw
        .get("check_runs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for run in &check_runs {
        let status = run
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let conclusion = run
            .get("conclusion")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if matches!(
            conclusion,
            "failure" | "timed_out" | "cancelled" | "action_required"
        ) {
            failed += 1;
        } else if status != "completed" || conclusion.is_empty() {
            pending += 1;
        } else if matches!(conclusion, "success" | "neutral" | "skipped") {
            passed += 1;
        } else {
            failed += 1;
        }
    }

    let status = if failed > 0 {
        "failed"
    } else if pending > 0 || check_runs.is_empty() {
        "pending"
    } else {
        "passed"
    };

    json!({
        "status": status,
        "passed": passed,
        "pending": pending,
        "failed": failed,
        "total": check_runs.len(),
        "raw": raw
    })
}

pub(crate) fn json_response(output: Value) -> Result<TaskExecutionResult, SendableError> {
    Ok(TaskExecutionResult {
        message: Some("github action completed".into()),
        output_json: Some(output.into()),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

pub(crate) fn auth_param() -> ParameterMetadata {
    ParameterMetadata::optional("token", RuninatorType::String).secret()
}

pub(crate) fn repo_owner_param() -> ParameterMetadata {
    ParameterMetadata::required("owner", RuninatorType::String)
}

pub(crate) fn repo_param() -> ParameterMetadata {
    ParameterMetadata::required("repo", RuninatorType::String)
}

pub(crate) fn json_results() -> Vec<ResultMetadata> {
    vec![
        ResultMetadata::new("response", RuninatorType::Any)
            .with_description("Raw GitHub API response body."),
    ]
}

pub(crate) fn pull_request_results() -> Vec<ResultMetadata> {
    vec![
        ResultMetadata::new("number", RuninatorType::Integer)
            .with_description("Pull request number."),
        ResultMetadata::new("html_url", RuninatorType::String)
            .with_description("Pull request web URL."),
        ResultMetadata::new(
            "head",
            RuninatorType::open_structure(
                [
                    ("sha", RuninatorType::String),
                    ("ref", RuninatorType::String),
                ],
                RuninatorType::Any,
            ),
        )
        .with_description("Pull request head reference."),
        ResultMetadata::new("response", RuninatorType::Any)
            .with_description("Raw GitHub API response body."),
    ]
}
