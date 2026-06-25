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

    let attachments = send_message
        .parameters
        .iter()
        .find(|parameter| parameter.name == "attachments")
        .expect("attachments parameter is advertised");
    // attachments is now a typed array of attachment structs, not a free-form map.
    let RuninatorType::Array(element) = &attachments.ty else {
        panic!("attachments should be an array, got {:?}", attachments.ty);
    };
    let RuninatorType::Struct { fields, .. } = element.as_ref() else {
        panic!("attachment element should be a struct, got {element:?}");
    };
    assert!(fields.contains_key("color"));
    assert!(fields.contains_key("fields"));

    assert!(
        send_message
            .parameters
            .iter()
            .any(|parameter| parameter.name == "token" && parameter.secret)
    );
}

#[test]
fn metadata_advertises_read_actions() {
    let provider = SlackProvider;
    let functions: Vec<String> = provider
        .metadata()
        .actions
        .into_iter()
        .map(|action| action.function_name)
        .collect();

    for expected in [
        "conversations_list",
        "conversations_history",
        "conversations_replies",
        "conversations_info",
        "users_info",
        "search_messages",
    ] {
        assert!(
            functions.iter().any(|name| name == expected),
            "missing read action {expected}"
        );
    }
}

#[test]
fn read_query_enforces_required_and_renders_scalars() {
    use crate::read::{build_query, find_action};

    let history = find_action("conversations_history").expect("action exists");

    // missing required `channel` is rejected.
    assert!(build_query(history, &json!({})).is_err());

    // present params render as string query pairs (ints/bools coerced).
    let query = build_query(
        history,
        &json!({ "channel": "C123", "limit": 50, "inclusive": true }),
    )
    .expect("query builds");
    assert!(query.contains(&("channel".to_string(), "C123".to_string())));
    assert!(query.contains(&("limit".to_string(), "50".to_string())));
    assert!(query.contains(&("inclusive".to_string(), "true".to_string())));

    // a comma-separated list param renders joined.
    let list = find_action("conversations_list").expect("action exists");
    let query = build_query(
        list,
        &json!({ "types": ["public_channel", "private_channel"] }),
    )
    .expect("query builds");
    assert!(query.contains(&(
        "types".to_string(),
        "public_channel,private_channel".to_string()
    )));
}

#[test]
fn rejects_mistyped_attachment_field() {
    // `short` must be a boolean; a string should fail typed validation.
    let result = build_send_message_payload(SendMessageParams {
        token: "xoxb-token".into(),
        channel: "C123".into(),
        text: "hello".into(),
        attachments: Some(json!([{ "fields": [{ "title": "k", "short": "yes" }] }])),
        blocks: None,
        thread_ts: None,
        mrkdwn: None,
        unfurl_links: None,
        unfurl_media: None,
    });
    assert!(result.is_err());
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
