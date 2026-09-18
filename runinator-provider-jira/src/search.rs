use runinator_jira::{JiraClient, JiraOperation};
use runinator_models::json;
use runinator_models::{errors::SendableError, runs::TaskExecutionResult};
use runinator_provider_support::polling::{PollPage, poll_pages};
use serde_json::Value;

use crate::error::client_error;
use crate::params::JiraSearchParams;

// paginates Jira's enhanced JQL search endpoint via nextPageToken and
// returns every issue aggregated into a single output.
pub(crate) fn jira_search_all(
    client: &JiraClient,
    p: &JiraSearchParams,
) -> Result<TaskExecutionResult, SendableError> {
    let issues = poll_pages(
        None,
        100,
        |next_page_token| {
            let page = client
                .execute(JiraOperation::Search {
                    jql: p.jql.clone(),
                    fields: "*all".into(),
                    max_results: None,
                    next_page_token,
                })
                .map_err(|error| client_error("jira search request failed", error))?;
            let items = page
                .get("issues")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let is_last = page.get("isLast").and_then(Value::as_bool).unwrap_or(false);
            let next_cursor = (!is_last)
                .then(|| {
                    page.get("nextPageToken")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .flatten();
            Ok::<_, SendableError>(PollPage { items, next_cursor })
        },
        |limit| crate::error::HTTP_ERROR.error(format!("jira search pagination failed: {limit:?}")),
    )?;

    let total = issues.len();
    Ok(TaskExecutionResult {
        message: Some(format!("jira search returned {total} issues")),
        output_json: Some(json!({ "issues": issues, "total": total })),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}
