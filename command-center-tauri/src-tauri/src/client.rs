use reqwest::Url;
use runinator_models::web::TaskResponse;
use serde_json::{json, Value};

use crate::{
    error::{CommandError, CommandResult},
    state::CommandCenterState,
};

pub async fn get_json<T>(state: &CommandCenterState, path: &str) -> CommandResult<T>
where
    T: serde::de::DeserializeOwned,
{
    let url = build_state_url(state, path).await?;
    let response = state.client.get(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<T>().await?)
}

pub async fn post_empty(state: &CommandCenterState, path: &str) -> CommandResult<TaskResponse> {
    let url = build_state_url(state, path).await?;
    let response = state
        .client
        .post(url.clone())
        .json(&json!({}))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

pub async fn build_state_url(state: &CommandCenterState, path: &str) -> CommandResult<Url> {
    let base = state
        .service_url
        .read()
        .await
        .clone()
        .ok_or(CommandError::NoService)?;
    build_url(&base, path)
}

pub fn build_url(base_url: &str, path: &str) -> CommandResult<Url> {
    let mut url = Url::parse(base_url)?;
    let trimmed = path.trim_start_matches('/');
    let (path_part, query) = trimmed
        .split_once('?')
        .map(|(left, right)| (left, Some(right)))
        .unwrap_or((trimmed, None));
    let mut base_path = url.path().to_string();
    if !base_path.ends_with('/') {
        base_path.push('/');
    }
    url.set_path(&(base_path + path_part));
    url.set_query(query);
    Ok(url)
}

pub async fn handle_response(
    url: Url,
    response: reqwest::Response,
) -> CommandResult<reqwest::Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let message = extract_error_message(&body).unwrap_or_else(|| {
        if body.trim().is_empty() {
            format!("{url} returned {status}")
        } else {
            body
        }
    });
    Err(CommandError::Unexpected(message))
}

fn extract_error_message(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .filter(|message| !message.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_url_preserves_query() {
        let url = build_url("http://localhost:3000/api/", "runs/7/chunks?limit=500").unwrap();
        assert_eq!(
            url.as_str(),
            "http://localhost:3000/api/runs/7/chunks?limit=500"
        );
    }

    #[test]
    fn extracts_json_error_message() {
        assert_eq!(
            extract_error_message(r#"{"message":"failed cleanly"}"#),
            Some("failed cleanly".to_string())
        );
    }

    #[test]
    fn ignores_empty_json_error_message() {
        assert_eq!(extract_error_message(r#"{"message":""}"#), None);
    }
}
