use std::sync::Arc;
use std::time::Duration;

mod errors;
mod read;

use runinator_models::json;
use runinator_models::value::{Map, Value};
use runinator_models::{
    errors::SendableError,
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata,
        ResultMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
    types::RuninatorField,
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

// typed shape of a Slack message attachment, used to validate the `attachments`
// parameter at runtime. unknown fields are ignored (Slack accepts more), so this
// type-checks the known fields without rejecting forward-compatible ones. the
// matching metadata schema is attachment_type().
#[allow(dead_code)]
#[derive(Deserialize)]
struct SlackAttachment {
    fallback: Option<String>,
    color: Option<String>,
    pretext: Option<String>,
    author_name: Option<String>,
    author_link: Option<String>,
    author_icon: Option<String>,
    title: Option<String>,
    title_link: Option<String>,
    text: Option<String>,
    fields: Option<Vec<AttachmentField>>,
    image_url: Option<String>,
    thumb_url: Option<String>,
    footer: Option<String>,
    footer_icon: Option<String>,
    ts: Option<i64>,
    mrkdwn_in: Option<Vec<String>>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct AttachmentField {
    title: Option<String>,
    value: Option<String>,
    short: Option<bool>,
}

#[derive(Clone)]
pub struct SlackProvider;

impl Provider for SlackProvider {
    fn name(&self) -> String {
        "slack".into()
    }

    fn metadata(&self) -> ProviderMetadata {
        let mut actions = vec![
            ActionMetadata::new("send_message", "Send a Slack message")
                .with_parameters(vec![
                    token_param(),
                    ParameterMetadata::required("channel", RuninatorType::String),
                    ParameterMetadata::required("text", RuninatorType::String),
                    ParameterMetadata::optional(
                        "attachments",
                        RuninatorType::array(attachment_type()),
                    )
                    .with_description("Slack attachment array (typed)."),
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
        ];
        actions.extend(read::read_action_metadata());

        ProviderMetadata {
            name: self.name(),
            actions,
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
        match request.action_function.as_str() {
            "send" | "send_message" => send_message(request),
            other => match read::find_action(other) {
                Some(def) => read::execute_read(def, &request),
                None => Err(UNSUPPORTED_ACTION.error(other)),
            },
        }
    }
}

fn send_message(request: ProviderExecutionRequest) -> Result<TaskExecutionResult, SendableError> {
    let params: SendMessageParams = parse_params(&request)?;
    let token = params.token.clone();
    let payload = build_send_message_payload(params)?;
    let client = build_client(request.timeout_secs)?;
    let response = client
        .post("https://slack.com/api/chat.postMessage")
        .bearer_auth(token)
        .header("Accept", "application/json")
        .json(&payload)
        .send()?;
    let output = parse_slack_ok(response)?;
    Ok(TaskExecutionResult {
        message: Some("slack message sent".into()),
        output_json: Some(output),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

// shared blocking client honoring the request timeout.
pub(crate) fn build_client(timeout_secs: i64) -> Result<reqwest::blocking::Client, SendableError> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.max(1) as u64))
        .user_agent("runinator")
        .build()
        .map_err(|err| -> SendableError { Box::new(err) })
}

runinator_provider_support::provider_parse_params!(crate::errors::INVALID_PARAMS);

fn build_send_message_payload(params: SendMessageParams) -> Result<Value, SendableError> {
    let mut payload = Map::new();
    payload.insert("channel".into(), json!(params.channel));
    payload.insert("text".into(), json!(params.text));

    if let Some(attachments) = params.attachments {
        validate_attachments(&attachments)?;
        payload.insert("attachments".into(), attachments);
    }
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

// validates a Slack Web API response (HTTP status + the JSON `ok` field) and
// returns the parsed body. shared by the send and read paths.
pub(crate) fn parse_slack_ok(
    response: reqwest::blocking::Response,
) -> Result<Value, SendableError> {
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
    Ok(output)
}

pub(crate) fn token_param() -> ParameterMetadata {
    ParameterMetadata::required("token", RuninatorType::String).secret()
}

// type-checks the attachments array against SlackAttachment, accepting unknown
// fields. errors carry the serde message so the offending field is identifiable.
fn validate_attachments(value: &Value) -> Result<(), SendableError> {
    let bridged = serde_json::to_value(value)
        .map_err(|err| INVALID_PARAMS.error(format!("attachments: {err}")))?;
    serde_json::from_value::<Vec<SlackAttachment>>(bridged)
        .map_err(|err| INVALID_PARAMS.error(format!("attachments: {err}")))?;
    Ok(())
}

// metadata schema mirroring SlackAttachment; open (additional Any) so extra Slack
// fields remain valid while the known ones are typed for the WDL/command-center.
fn attachment_type() -> RuninatorType {
    RuninatorType::open_typed_structure(
        [
            ("fallback", RuninatorField::optional(RuninatorType::String)),
            ("color", RuninatorField::optional(RuninatorType::String)),
            ("pretext", RuninatorField::optional(RuninatorType::String)),
            (
                "author_name",
                RuninatorField::optional(RuninatorType::String),
            ),
            (
                "author_link",
                RuninatorField::optional(RuninatorType::String),
            ),
            (
                "author_icon",
                RuninatorField::optional(RuninatorType::String),
            ),
            ("title", RuninatorField::optional(RuninatorType::String)),
            (
                "title_link",
                RuninatorField::optional(RuninatorType::String),
            ),
            ("text", RuninatorField::optional(RuninatorType::String)),
            (
                "fields",
                RuninatorField::optional(RuninatorType::array(attachment_field_type())),
            ),
            ("image_url", RuninatorField::optional(RuninatorType::String)),
            ("thumb_url", RuninatorField::optional(RuninatorType::String)),
            ("footer", RuninatorField::optional(RuninatorType::String)),
            (
                "footer_icon",
                RuninatorField::optional(RuninatorType::String),
            ),
            ("ts", RuninatorField::optional(RuninatorType::Integer)),
            (
                "mrkdwn_in",
                RuninatorField::optional(RuninatorType::array(RuninatorType::String)),
            ),
        ],
        RuninatorType::Any,
    )
}

fn attachment_field_type() -> RuninatorType {
    RuninatorType::open_typed_structure(
        [
            ("title", RuninatorField::optional(RuninatorType::String)),
            ("value", RuninatorField::optional(RuninatorType::String)),
            ("short", RuninatorField::optional(RuninatorType::Boolean)),
        ],
        RuninatorType::Any,
    )
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
