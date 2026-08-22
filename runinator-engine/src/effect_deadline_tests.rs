//! covers the action deadline backstop: which effects get one, when it fires, and what it settles.

use super::*;

use runinator_comm::{EffectExecutor, EffectResultKind};
use runinator_models::workflow_vm::WORKFLOW_EFFECT_PROTOCOL_VERSION;
use uuid::Uuid;

fn command(request: WorkflowEffectRequest, attempt: u32) -> EffectCommand {
    EffectCommand {
        version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
        command_id: Uuid::now_v7(),
        effect_id: Uuid::now_v7(),
        workflow_run_id: Uuid::now_v7(),
        continuation_id: Uuid::now_v7(),
        attempt,
        request,
        executor: EffectExecutor::Provider,
        target: Default::default(),
        trace_id: Uuid::now_v7(),
        trace_context: Default::default(),
        idempotency_key: Uuid::now_v7().to_string(),
        notification_delivery_id: None,
    }
}

fn action(timeout_seconds: Option<i64>) -> WorkflowEffectRequest {
    WorkflowEffectRequest::Action {
        provider: "http".into(),
        function: "get".into(),
        input: Default::default(),
        timeout_seconds,
        retry: Default::default(),
        tags: Default::default(),
        required_labels: Default::default(),
        idempotency_key: None,
        function_binding: None,
    }
}

#[test]
fn a_declared_timeout_fires_a_grace_period_after_the_workers_own_deadline() {
    let dispatched_at = Utc::now();
    let wake = deadline_wake(&command(action(Some(120)), 0), dispatched_at)
        .expect("an action should get a deadline");

    // later than the worker's own 120s deadline, so a live worker always reports first and the
    // backstop only lands when nothing answered at all.
    assert_eq!(
        (wake.due_at - dispatched_at).num_seconds(),
        120 + DEADLINE_GRACE_SECONDS
    );
}

#[test]
fn an_undeclared_timeout_uses_the_same_default_the_worker_applies() {
    let dispatched_at = Utc::now();
    let wake = deadline_wake(&command(action(None), 0), dispatched_at)
        .expect("an action without a declared timeout still gets a deadline");

    // the majority of actions declare no timeout; covering only declared ones would leave them
    // parked forever behind a dead worker.
    assert_eq!(
        (wake.due_at - dispatched_at).num_seconds(),
        DEFAULT_ACTION_TIMEOUT_SECONDS + DEADLINE_GRACE_SECONDS
    );
}

#[test]
fn a_nonpositive_timeout_still_yields_a_future_deadline() {
    let dispatched_at = Utc::now();
    let wake = deadline_wake(&command(action(Some(0)), 0), dispatched_at).expect("still an action");

    // the worker clamps with `.max(1)`; a deadline at or before dispatch would settle an action
    // the instant it was handed out.
    assert!(wake.due_at > dispatched_at);
}

#[test]
fn the_deadline_settles_timed_out_and_is_stamped_at_the_due_instant() {
    let dispatched_at = Utc::now();
    let command = command(action(Some(30)), 0);
    let wake = deadline_wake(&command, dispatched_at).expect("an action should get a deadline");

    assert_eq!(wake.result.effect_id, command.effect_id);
    assert_eq!(wake.result.timestamp, wake.due_at);
    match &wake.result.kind {
        EffectResultKind::Status {
            status, message, ..
        } => {
            assert_eq!(*status, WorkflowEffectStatus::TimedOut);
            // the message has to be distinguishable from the worker's own timeout report: one
            // means the action overran, the other that nothing ever answered.
            assert!(
                message
                    .as_deref()
                    .is_some_and(|message| message.contains("never reported")),
                "unexpected deadline message: {message:?}"
            );
        }
        other => panic!("expected a status result, got {other:?}"),
    }
}

#[test]
fn each_attempt_arms_its_own_deadline() {
    let dispatched_at = Utc::now();
    let first = command(action(Some(30)), 0);
    let mut retried = first.clone();
    retried.attempt = 1;

    let first = deadline_wake(&first, dispatched_at).expect("an action should get a deadline");
    let retried = deadline_wake(&retried, dispatched_at).expect("a retry gets its own deadline");

    // a retry must not be suppressed by the previous attempt's still-armed wake, and a redelivered
    // dispatch of the same attempt must be.
    assert_ne!(first.dedupe_key(), retried.dedupe_key());
}

#[test]
fn only_provider_actions_get_a_deadline() {
    let dispatched_at = Utc::now();

    // a timer is its own deadline; a signal parks by design and has none to enforce. arming either
    // here would settle an effect the infrastructure host or an operator already owns.
    for request in [
        WorkflowEffectRequest::Timer { due_at: 0 },
        WorkflowEffectRequest::TimerDelay { seconds: 30 },
        WorkflowEffectRequest::Signal {
            key: "release".into(),
            filter: None,
        },
        WorkflowEffectRequest::MutexAcquire { key: "lock".into() },
    ] {
        assert!(
            deadline_wake(&command(request.clone(), 0), dispatched_at).is_none(),
            "{request:?} should not get an action deadline"
        );
    }
}

#[tokio::test]
async fn arming_publishes_one_wake_per_attempt_and_none_for_a_non_action() {
    use runinator_broker_core::in_memory::InMemoryBroker;
    use std::sync::Arc;

    let broker: Arc<dyn Broker> = Arc::new(InMemoryBroker::new());
    let dispatched_at = Utc::now();
    let action = command(action(Some(45)), 0);

    arm(broker.as_ref(), &action, dispatched_at).await;
    let delivery = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        broker.receive_wake("waker"),
    )
    .await
    .expect("arming should publish a wake")
    .unwrap();
    assert_eq!(delivery.command.effect_id(), action.effect_id);
    assert_eq!(
        (delivery.command.due_at - dispatched_at).num_seconds(),
        45 + DEADLINE_GRACE_SECONDS
    );

    // a timer owns its own deadline, so arming beside it would settle it twice.
    arm(
        broker.as_ref(),
        &command(WorkflowEffectRequest::Timer { due_at: 0 }, 0),
        dispatched_at,
    )
    .await;
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(200),
            broker.receive_wake("waker")
        )
        .await
        .is_err(),
        "a non-action must not arm a deadline"
    );
}
