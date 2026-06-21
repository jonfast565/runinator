use runinator_models::{
    errors::SendableError,
    providers::{ParameterMetadata, ResultMetadata, RuninatorType},
    runs::TaskExecutionResult,
};
use serde_json::{Value, json};

use crate::errors::{HTTP_ERROR, INVALID_JSON};

runinator_provider_support::provider_parse_params!(crate::errors::INVALID_PARAMS);

pub(crate) fn first_pull_number(
    response: reqwest::blocking::Response,
) -> Result<Option<i64>, SendableError> {
    let text = response.text()?;
    let value: Value = serde_json::from_str(&text).map_err(|err| {
        INVALID_JSON.error(format!(
            "GitHub pull request list response was not JSON: {err}"
        ))
    })?;
    Ok(value
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item.get("number"))
        .and_then(Value::as_i64))
}

pub(crate) fn checks_summary_response(
    response: reqwest::blocking::Response,
) -> Result<TaskExecutionResult, SendableError> {
    let status = response.status();
    let text = response.text()?;
    if !status.is_success() {
        return Err(HTTP_ERROR.error(format!("HTTP {status}: {text}")));
    }
    let raw: Value = serde_json::from_str(&text)
        .unwrap_or_else(|_| json!({ "body": text, "status": status.as_u16() }));
    let summary = summarize_check_runs(raw);
    Ok(TaskExecutionResult {
        message: Some("github checks summary completed".into()),
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
        message: Some("github action completed".into()),
        output_json: Some(output.into()),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

pub(crate) fn auth_param() -> ParameterMetadata {
    ParameterMetadata::required("token", RuninatorType::String).secret()
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
