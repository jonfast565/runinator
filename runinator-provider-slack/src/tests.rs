use super::*;
use runinator_models::runs::ProviderExecutionRequest;

#[test]
fn missing_token_is_invalid() {
    let provider = SlackProvider;
    let request = ProviderExecutionRequest {
        run_id: Some(uuid::Uuid::now_v7()),
        action_name: "slack".into(),
        action_function: "send_message".into(),
        parameters: json!({
            "channel": "C123",
            "text": "hello"
        }),
        timeout_secs: 30,
        artifact_dir: "".into(),
        events_jsonl_path: "".into(),
    };

    let result = provider.execute_service(
        request,
        None,
        runinator_plugin::cancel::CancellationToken::new(),
    );
    assert!(result.is_err());
}

#[test]
fn metadata_advertises_send_message_with_attachments() {
    let provider = SlackProvider;
    let metadata = provider.metadata();

    let send_message = metadata
        .actions
        .iter()
        .find(|action| action.function_name == "send_message")
        .expect("send_message action is advertised");

    assert!(send_message.parameters.iter().any(|parameter| {
        parameter.name == "attachments"
            && parameter.ty == RuninatorType::array(RuninatorType::map(RuninatorType::Any))
    }));
    assert!(
        send_message
            .parameters
            .iter()
            .any(|parameter| parameter.name == "token" && parameter.secret)
    );
}

#[test]
fn builds_payload_with_optional_fields() {
    let payload = build_send_message_payload(SendMessageParams {
        token: "xoxb-token".into(),
        channel: "C123".into(),
        text: "hello".into(),
        attachments: Some(json!([{ "color": "#36a64f", "text": "details" }])),
        blocks: Some(json!([{ "type": "section", "text": { "type": "mrkdwn", "text": "hello" } }])),
        thread_ts: Some("1712345678.000100".into()),
        mrkdwn: Some(true),
        unfurl_links: Some(false),
        unfurl_media: Some(false),
    })
    .expect("payload should build");

    assert_eq!(payload["channel"], "C123");
    assert_eq!(payload["attachments"][0]["text"], "details");
    assert_eq!(payload["blocks"][0]["type"], "section");
    assert_eq!(payload["thread_ts"], "1712345678.000100");
    assert_eq!(payload["unfurl_links"], false);
}

#[test]
fn rejects_non_array_attachments() {
    let result = build_send_message_payload(SendMessageParams {
        token: "xoxb-token".into(),
        channel: "C123".into(),
        text: "hello".into(),
        attachments: Some(json!({ "text": "details" })),
        blocks: None,
        thread_ts: None,
        mrkdwn: None,
        unfurl_links: None,
        unfurl_media: None,
    });

    assert!(result.is_err());
}
