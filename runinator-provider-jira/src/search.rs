use runinator_models::json;
use runinator_models::value::Value;
use runinator_models::{errors::SendableError, runs::TaskExecutionResult};

use crate::error::{http_error, validate_base_url};
use crate::params::JiraSearchParams;
use crate::response::json_response;

// paginates the new /rest/api/3/search/jql endpoint via nextPageToken and
// returns every issue aggregated into a single output.
pub(crate) fn jira_search_all(
    client: &reqwest::blocking::Client,
    p: &JiraSearchParams,
) -> Result<TaskExecutionResult, SendableError> {
    validate_base_url(&p.base.base_url)?;
    let url = format!("{}/rest/api/3/search/jql", p.base.base_url);
    let auth_user = p.base.email.as_deref().unwrap_or_default();
    let mut issues: Vec<Value> = Vec::new();
    let mut next_token: Option<String> = None;

    loop {
        let mut query: Vec<(&str, String)> =
            vec![("jql", p.jql.clone()), ("fields", "*all".to_string())];
        if let Some(token) = next_token.as_ref() {
            query.push(("nextPageToken", token.clone()));
        }

        let response = client
            .get(&url)
            .query(&query)
            .basic_auth(auth_user, Some(&p.base.token))
            .send()
            .map_err(|e| http_error("jira search request failed", e))?;

        let mut result = json_response(response)?;
        let page = result.output_json.take().unwrap_or(Value::Null);

        if let Some(arr) = page.get("issues").and_then(Value::as_array) {
            issues.extend(arr.iter().cloned());
        }

        let is_last = page.get("isLast").and_then(Value::as_bool).unwrap_or(false);
        next_token = page
            .get("nextPageToken")
            .and_then(Value::as_str)
            .map(str::to_string);

        if is_last || next_token.is_none() {
            break;
        }
    }

    let total = issues.len();
    Ok(TaskExecutionResult {
        message: Some(format!("jira search returned {total} issues")),
        output_json: Some(json!({ "issues": issues, "total": total })),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}
