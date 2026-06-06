use std::sync::Arc;
use std::time::Duration;

mod errors;

use runinator_models::json;
use runinator_models::value::{Map, Value};
use runinator_models::{
    errors::SendableError,
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata,
        ResultMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde::Deserialize;

use crate::errors::{API_ERROR, HTTP_ERROR, INVALID_JSON, INVALID_PARAMS, UNSUPPORTED_ACTION};

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
                        ParameterMetadata::optional(
                            "attachments",
                            RuninatorType::array(RuninatorType::map(RuninatorType::Any)),
                        )
                        .with_description("Slack attachment array."),
                        ParameterMetadata::optional(
                            "blocks",
                            RuninatorType::array(RuninatorType::map(RuninatorType::Any)),
                        )
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
                return Err(UNSUPPORTED_ACTION.error(other));
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
    serde_json::from_value(request.parameters.clone().into())
        .map_err(|err| INVALID_PARAMS.error(err))
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
    payload: &mut Map,
    name: &str,
    value: Option<Value>,
) -> Result<(), SendableError> {
    let Some(value) = value else {
        return Ok(());
    };
    if !value.is_array() {
        return Err(INVALID_PARAMS.error(format!("{name} must be a JSON array")));
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
        return Err(HTTP_ERROR.error(format!("HTTP {status}: {text}")));
    }

    let output: Value = serde_json::from_str(&text)
        .map_err(|err| INVALID_JSON.error(format!("Slack response was not JSON: {err}")))?;
    let ok = output.get("ok").and_then(Value::as_bool).unwrap_or(false);
    if !ok {
        let message = output
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Slack API returned ok=false");
        return Err(API_ERROR.error(message));
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
        ResultMetadata::new("ok", RuninatorType::Boolean),
        ResultMetadata::new("channel", RuninatorType::String),
        ResultMetadata::new("ts", RuninatorType::String),
        ResultMetadata::new("message", RuninatorType::map(RuninatorType::Any)),
    ]
}

#[cfg(test)]
mod tests;
