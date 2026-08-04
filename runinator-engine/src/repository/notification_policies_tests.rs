use super::*;
use runinator_models::notifications::NotificationSeverity;

fn base() -> NewNotificationPolicy {
    NewNotificationPolicy {
        workflow_id: None,
        name: "oncall".into(),
        event: NotificationEvent::RunFailed,
        severity: NotificationSeverity::Critical,
        channel: NotificationChannel::Slack,
        target: Some("#oncall".into()),
        threshold_seconds: None,
        enabled: true,
        managed_by: None,
        configuration: Value::Null,
    }
}

#[test]
fn a_well_formed_policy_validates() {
    assert!(validate_policy(&base()).is_ok());
}

#[test]
fn an_external_channel_requires_a_target() {
    let mut policy = base();
    policy.target = None;
    assert!(validate_policy(&policy).is_err());
    // whitespace is not a target either.
    policy.target = Some("   ".into());
    assert!(validate_policy(&policy).is_err());
}

#[test]
fn an_in_app_policy_needs_no_target() {
    let mut policy = base();
    policy.channel = NotificationChannel::InApp;
    policy.target = None;
    assert!(validate_policy(&policy).is_ok());
}

#[test]
fn a_duration_event_requires_a_positive_threshold() {
    let mut policy = base();
    policy.event = NotificationEvent::RunSlaBreached;
    // inert without a threshold: the scanner would never match it.
    assert!(validate_policy(&policy).is_err());
    policy.threshold_seconds = Some(0);
    assert!(validate_policy(&policy).is_err());
    policy.threshold_seconds = Some(1800);
    assert!(validate_policy(&policy).is_ok());
}

#[test]
fn a_transition_event_needs_no_threshold() {
    let mut policy = base();
    policy.event = NotificationEvent::NodeRetryExhausted;
    assert!(validate_policy(&policy).is_ok());
}

#[test]
fn a_policy_requires_a_name() {
    let mut policy = base();
    policy.name = "  ".into();
    assert!(validate_policy(&policy).is_err());
}
