//! covers folding worker events into the agent's counters.

use super::*;

use uuid::Uuid;

fn started() -> WorkerEvent {
    WorkerEvent::ActionStarted {
        workflow_run_id: Uuid::nil(),
        node_id: "step".to_string(),
        node_run_id: Uuid::nil(),
        provider: "console".to_string(),
        function: "run".to_string(),
        attempt: 1,
    }
}

fn finished(outcome: ActionOutcome) -> WorkerEvent {
    WorkerEvent::ActionFinished {
        workflow_run_id: Uuid::nil(),
        node_id: "step".to_string(),
        node_run_id: Uuid::nil(),
        provider: "console".to_string(),
        function: "run".to_string(),
        outcome,
        duration_ms: 12,
        message: None,
    }
}

#[test]
fn in_flight_rises_on_start_and_falls_on_finish() {
    let mut metrics = AgentMetrics::default();
    metrics.apply(&started());
    metrics.apply(&started());
    assert_eq!(metrics.in_flight, 2);
    metrics.apply(&finished(ActionOutcome::Succeeded));
    assert_eq!(metrics.in_flight, 1);
    assert_eq!(metrics.succeeded, 1);
}

#[test]
fn each_outcome_lands_in_its_own_counter() {
    let mut metrics = AgentMetrics::default();
    for outcome in [
        ActionOutcome::Succeeded,
        ActionOutcome::Failed,
        ActionOutcome::TimedOut,
        ActionOutcome::Canceled,
    ] {
        metrics.apply(&finished(outcome));
    }
    assert_eq!(
        (
            metrics.succeeded,
            metrics.failed,
            metrics.timed_out,
            metrics.canceled
        ),
        (1, 1, 1, 1)
    );
}

// a supervised restart can deliver a finish whose start belonged to the previous attempt. the
// counter must floor at zero rather than wrap to u32::MAX and report a permanently busy agent.
#[test]
fn a_finish_without_a_matching_start_does_not_wrap_in_flight() {
    let mut metrics = AgentMetrics::default();
    metrics.apply(&finished(ActionOutcome::Failed));
    assert_eq!(metrics.in_flight, 0);
}

#[test]
fn the_last_completed_action_is_retained() {
    let mut metrics = AgentMetrics::default();
    metrics.apply(&finished(ActionOutcome::TimedOut));
    let last = metrics.last_completed.expect("a finish should be recorded");
    assert_eq!(last.outcome, ActionOutcome::TimedOut);
    assert_eq!(last.duration_ms, 12);
    assert!(last.summary.contains("console.run"), "{}", last.summary);
}

#[test]
fn a_duplicate_delivery_only_moves_its_own_counter() {
    let mut metrics = AgentMetrics::default();
    metrics.apply(&WorkerEvent::ActionSkippedDuplicate {
        node_run_id: Uuid::nil(),
    });
    assert_eq!(metrics.skipped_duplicates, 1);
    assert_eq!(metrics.in_flight, 0);
}
