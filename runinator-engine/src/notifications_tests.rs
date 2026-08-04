use super::*;
use runinator_models::notifications::{
    NotificationChannel, NotificationEvent, NotificationSeverity,
};

fn policy(channel: NotificationChannel, target: Option<&str>) -> NotificationPolicy {
    NotificationPolicy {
        id: Uuid::nil(),
        workflow_id: None,
        name: "oncall".into(),
        event: NotificationEvent::RunFailed,
        severity: NotificationSeverity::Critical,
        channel,
        target: target.map(|t| t.to_string()),
        threshold_seconds: None,
        enabled: true,
        managed_by: None,
        configuration: Value::Null,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

fn context() -> EmissionContext {
    EmissionContext {
        workflow_run_id: Some(Uuid::nil()),
        node_id: Some("build".into()),
        title: "nightly failed".into(),
        body: "it broke".into(),
        metadata: Value::Null,
        occurrence: "run_failed:x".into(),
    }
}

#[test]
fn slack_delivery_renders_channel_and_text() {
    let configuration = delivery_configuration(
        &policy(NotificationChannel::Slack, Some("#oncall")),
        "#oncall",
        &context(),
    );
    assert_eq!(
        configuration.get("channel").and_then(|v| v.as_str()),
        Some("#oncall")
    );
    let text = configuration
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(text.contains("nightly failed"), "title in text: {text}");
    assert!(text.contains("it broke"), "body in text: {text}");
    // the credential stays a late-resolved reference; the engine never reads the secret itself.
    assert_eq!(
        configuration.get("token").and_then(|v| v.as_str()),
        Some("secret://slack/bot_token")
    );
}

#[test]
fn email_delivery_renders_recipient_and_subject() {
    let configuration = delivery_configuration(
        &policy(NotificationChannel::Email, Some("ops@example.com")),
        "ops@example.com",
        &context(),
    );
    assert_eq!(
        configuration.get("to").and_then(|v| v.as_str()),
        Some("ops@example.com")
    );
    assert_eq!(
        configuration.get("subject").and_then(|v| v.as_str()),
        Some("nightly failed")
    );
}

#[test]
fn policy_configuration_overrides_generated_fields() {
    let mut custom = policy(NotificationChannel::Slack, Some("#oncall"));
    custom.configuration = runinator_models::json!({ "token": "secret://slack/alt" });
    let configuration = delivery_configuration(&custom, "#oncall", &context());
    assert_eq!(
        configuration.get("token").and_then(|v| v.as_str()),
        Some("secret://slack/alt")
    );
    // overriding one field must not drop the generated ones.
    assert_eq!(
        configuration.get("channel").and_then(|v| v.as_str()),
        Some("#oncall")
    );
}

#[test]
fn a_global_policy_covers_every_workflow() {
    let global = policy(NotificationChannel::InApp, None);
    assert!(policy_covers(&global, Uuid::now_v7()));
}

#[test]
fn a_scoped_policy_covers_only_its_workflow() {
    let workflow_id = Uuid::now_v7();
    let mut scoped = policy(NotificationChannel::InApp, None);
    scoped.workflow_id = Some(workflow_id);
    assert!(policy_covers(&scoped, workflow_id));
    assert!(!policy_covers(&scoped, Uuid::now_v7()));
}

#[test]
fn parked_covers_the_blocked_states_only() {
    assert!(is_parked(WorkflowStatus::Waiting));
    assert!(is_parked(WorkflowStatus::ApprovalRequired));
    assert!(is_parked(WorkflowStatus::InputRequired));
    assert!(is_parked(WorkflowStatus::Blocked));
    // a run doing work is late, not parked; `run_sla_breached` is the event for that.
    assert!(!is_parked(WorkflowStatus::Running));
    assert!(!is_parked(WorkflowStatus::Queued));
}

#[test]
fn durations_render_at_the_right_granularity() {
    assert_eq!(humanize_seconds(45), "45s");
    assert_eq!(humanize_seconds(90), "1m");
    assert_eq!(humanize_seconds(3660), "1h1m");
    assert_eq!(humanize_seconds(90000), "1d1h");
    // a clock skew that makes an age negative must not render as a nonsense duration.
    assert_eq!(humanize_seconds(-5), "0s");
}

#[test]
fn an_unroutable_channel_has_no_provider() {
    assert!(NotificationChannel::InApp.provider().is_none());
    assert_eq!(
        NotificationChannel::Slack.provider(),
        Some(("slack", "send_message"))
    );
    assert_eq!(
        NotificationChannel::Email.provider(),
        Some(("email", "send"))
    );
}
