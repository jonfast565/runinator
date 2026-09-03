//! Covers which terminals re-arm an effect, when the policy is exhausted, and the backoff curve.

use super::*;
use runinator_models::value::Value;
use runinator_models::workflows::WorkflowRetryClass;
use uuid::Uuid;

fn effect(attempt: u32, retry: WorkflowRetry) -> WorkflowEffect {
    WorkflowEffect {
        id: Uuid::now_v7(),
        version: runinator_models::workflow_vm::WORKFLOW_EFFECT_PROTOCOL_VERSION,
        workflow_run_id: Uuid::now_v7(),
        continuation_id: Uuid::now_v7(),
        sequence: 0,
        node_id: Some("node-a".into()),
        attempt,
        request: WorkflowEffectRequest::Action {
            provider: "test".into(),
            function: "execute".into(),
            input: Value::Null,
            timeout_seconds: Some(30),
            retry,
            tags: Vec::new(),
            required_labels: Default::default(),
            workspace_affinity: None,
            execution_profile: None,
            idempotency_key: None,
            function_binding: None,
        },
        status: WorkflowEffectStatus::Running,
        result: None,
        message: None,
        current_executor_replica_id: None,
        last_executor_replica_id: None,
        created_at: 0,
        updated_at: 0,
        finished_at: None,
    }
}

fn policy(max_attempts: i64) -> WorkflowRetry {
    WorkflowRetry {
        max_attempts,
        ..Default::default()
    }
}

#[test]
fn a_failure_under_the_attempt_budget_is_re_armed() {
    let now = Utc::now();
    let due = next_attempt_at(&effect(0, policy(3)), WorkflowEffectStatus::Failed, now);
    assert!(due.is_some());
}

#[test]
fn the_last_attempt_is_not_re_armed() {
    let now = Utc::now();
    // attempt 2 of `max_attempts: 3` is the third and final run.
    assert!(next_attempt_at(&effect(2, policy(3)), WorkflowEffectStatus::Failed, now).is_none());
}

#[test]
fn the_default_policy_never_retries() {
    let now = Utc::now();
    assert!(
        next_attempt_at(
            &effect(0, WorkflowRetry::default()),
            WorkflowEffectStatus::Failed,
            now
        )
        .is_none(),
        "max_attempts defaults to 1, so an un-annotated node must run exactly once"
    );
}

#[test]
fn a_rejection_or_cancel_is_never_retried() {
    let now = Utc::now();
    for status in [
        WorkflowEffectStatus::Rejected,
        WorkflowEffectStatus::Canceled,
        WorkflowEffectStatus::Succeeded,
    ] {
        assert!(
            next_attempt_at(&effect(0, policy(5)), status, now).is_none(),
            "{status:?} must not re-arm the effect"
        );
    }
}

#[test]
fn the_retry_on_class_filters_the_terminal() {
    let now = Utc::now();
    let failure_only = WorkflowRetry {
        max_attempts: 3,
        retry_on: WorkflowRetryClass::Failure,
        ..Default::default()
    };
    assert!(
        next_attempt_at(
            &effect(0, failure_only.clone()),
            WorkflowEffectStatus::TimedOut,
            now
        )
        .is_none()
    );
    assert!(next_attempt_at(&effect(0, failure_only), WorkflowEffectStatus::Failed, now).is_some());
}

#[test]
fn a_non_action_effect_has_no_retry_policy() {
    let now = Utc::now();
    let mut timer = effect(0, policy(5));
    timer.request = WorkflowEffectRequest::Timer { due_at: 1 };
    assert!(next_attempt_at(&timer, WorkflowEffectStatus::Failed, now).is_none());
}

#[test]
fn the_backoff_doubles_per_attempt_and_is_capped() {
    let retry = WorkflowRetry {
        max_attempts: 20,
        backoff_base_seconds: 2,
        backoff_max_seconds: 30,
        ..Default::default()
    };
    assert_eq!(backoff_seconds(&retry, 0), 2);
    assert_eq!(backoff_seconds(&retry, 1), 4);
    assert_eq!(backoff_seconds(&retry, 3), 16);
    assert_eq!(backoff_seconds(&retry, 4), 30, "capped");
    // a far-future attempt must saturate at the cap rather than overflow into a negative delay.
    assert_eq!(backoff_seconds(&retry, 63), 30);
}

#[test]
fn jitter_keeps_the_delay_in_the_upper_half() {
    let retry = WorkflowRetry {
        max_attempts: 5,
        backoff_base_seconds: 64,
        backoff_max_seconds: 300,
        jitter: true,
        ..Default::default()
    };
    for _ in 0..50 {
        let delay = backoff_seconds(&retry, 0);
        assert!(
            (32..=64).contains(&delay),
            "jittered delay {delay} out of range"
        );
    }
}
