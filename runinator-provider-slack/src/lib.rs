use std::sync::Arc;
use std::time::Duration;

use runinator_models::{
    errors::{RuntimeError, SendableError},
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata,
        ResultMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde::Deserialize;
use serde_json::{Map, Value, json};

#[derive(Deserialize)]
struct SendMessageParams {
    token: String,
    channel: String,
    text: String,
    attachments: Option<Value>,
    blocks: Option<Value>,
    thread_ts: Option<String>,
    mrkdwn: Option<bool>,
    unfurl_links: Option<bool>,
    unfurl_media: Option<bool>,
}

#[derive(Clone)]
pub struct SlackProvider;

impl Provider for SlackProvider {
    fn name(&self) -> String {
        "slack".into()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            actions: vec![
                ActionMetadata::new("send_message", "Send a Slack message")
                    .with_parameters(vec![
                        token_param(),
                        ParameterMetadata::required("channel", RuninatorType::String),
                        ParameterMetadata::required("text", RuninatorType::String),
                        ParameterMetadata::optional("attachments", RuninatorType::Any)
                            .with_description("Slack attachment array."),
                        ParameterMetadata::optional("blocks", RuninatorType::Any)
                            .with_description("Slack block kit array."),
                        ParameterMetadata::optional("thread_ts", RuninatorType::String),
                        ParameterMetadata::optional("mrkdwn", RuninatorType::Boolean),
                        ParameterMetadata::optional("unfurl_links", RuninatorType::Boolean),
                        ParameterMetadata::optional("unfurl_media", RuninatorType::Boolean),
                    ])
                    .with_results(slack_results()),
            ],
            metadata: ProviderRuntimeMetadata {
                credential_scopes: vec!["slack".into()],
                contract: None,
            },
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
        _token: runinator_plugin::cancel::CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        let function = request.action_function.as_str();
        let params: SendMessageParams = match function {
            "send" | "send_message" => parse_params(&request)?,
            other => {
                return Err(Box::new(RuntimeError::new(
                    "slack.unsupported_action".into(),
                    format!("Unsupported Slack action {other}"),
                )));
            }
        };
        let token = params.token.clone();
        let payload = build_send_message_payload(params)?;
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(request.timeout_secs.max(1) as u64))
            .user_agent("runinator")
            .build()?;
        let response = client
            .post("https://slack.com/api/chat.postMessage")
            .bearer_auth(token)
            .header("Accept", "application/json")
            .json(&payload)
            .send()?;
        slack_response(response)
    }
}

fn parse_params<T: serde::de::DeserializeOwned>(
    request: &ProviderExecutionRequest,
) -> Result<T, SendableError> {
    serde_json::from_value(request.parameters.clone()).map_err(|err| {
        Box::new(RuntimeError::new(
            "slack.invalid_params".into(),
            err.to_string(),
        )) as SendableError
    })
}

fn build_send_message_payload(params: SendMessageParams) -> Result<Value, SendableError> {
    let mut payload = Map::new();
    payload.insert("channel".into(), json!(params.channel));
    payload.insert("text".into(), json!(params.text));

    insert_array_param(&mut payload, "attachments", params.attachments)?;
    insert_array_param(&mut payload, "blocks", params.blocks)?;

    if let Some(thread_ts) = params.thread_ts {
        payload.insert("thread_ts".into(), json!(thread_ts));
    }
    if let Some(mrkdwn) = params.mrkdwn {
        payload.insert("mrkdwn".into(), json!(mrkdwn));
    }
    if let Some(unfurl_links) = params.unfurl_links {
        payload.insert("unfurl_links".into(), json!(unfurl_links));
    }
    if let Some(unfurl_media) = params.unfurl_media {
        payload.insert("unfurl_media".into(), json!(unfurl_media));
    }

    Ok(Value::Object(payload))
}

fn insert_array_param(
    payload: &mut Map<String, Value>,
    name: &str,
    value: Option<Value>,
) -> Result<(), SendableError> {
    let Some(value) = value else {
        return Ok(());
    };
    if !value.is_array() {
        return Err(Box::new(RuntimeError::new(
            "slack.invalid_params".into(),
            format!("{name} must be a JSON array"),
        )));
    }
    payload.insert(name.into(), value);
    Ok(())
}

fn slack_response(
    response: reqwest::blocking::Response,
) -> Result<TaskExecutionResult, SendableError> {
    let status = response.status();
    let text = response.text()?;
    if !status.is_success() {
        return Err(Box::new(RuntimeError::new(
            "slack.http_error".into(),
            format!("HTTP {status}: {text}"),
        )));
    }

    let output: Value = serde_json::from_str(&text).map_err(|err| {
        RuntimeError::new(
            "slack.invalid_json".into(),
            format!("Slack response was not JSON: {err}"),
        )
    })?;
    let ok = output.get("ok").and_then(Value::as_bool).unwrap_or(false);
    if !ok {
        let message = output
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Slack API returned ok=false");
        return Err(Box::new(RuntimeError::new(
            "slack.api_error".into(),
            message.to_string(),
        )));
    }

    Ok(TaskExecutionResult {
        message: Some("slack message sent".into()),
        output_json: Some(output),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn token_param() -> ParameterMetadata {
    ParameterMetadata::required("token", RuninatorType::String).secret()
}

fn slack_results() -> Vec<ResultMetadata> {
    vec![
        ResultMetadata::new("response", RuninatorType::Any)
            .with_description("Raw Slack chat.postMessage response body."),
    ]
}

#[cfg(test)]
mod tests;
