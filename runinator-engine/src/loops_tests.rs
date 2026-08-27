use std::{collections::BTreeMap, time::Duration};

use runinator_models::{
    orchestration::{BudgetExhaustion, BudgetPolicy},
    pipelines::{PipelineMemberAttempt, PipelineMemberAttemptStatus},
    value::Value,
    workflows::WorkflowStatus,
    workspaces::{WorkspaceAffinity, WorkspaceLease, WorkspaceStatus},
};
use uuid::Uuid;

use super::{
    FailureBudgetDecision, bucket_to_interval, consume_failure_budget, select_epoch_phase_attempt,
    workspace_affinity_matches,
};

fn pipeline_attempt(
    member: &str,
    status: PipelineMemberAttemptStatus,
    offset_seconds: i64,
) -> PipelineMemberAttempt {
    let created_at = chrono::DateTime::from_timestamp(1_700_000_100 + offset_seconds, 0).unwrap();
    PipelineMemberAttempt {
        id: Uuid::now_v7(),
        pipeline_run_id: Uuid::now_v7(),
        member_key: member.into(),
        workflow_id: Uuid::now_v7(),
        attempt: 1,
        workflow_run_id: None,
        status,
        parameters: Value::Null,
        result: Value::Null,
        message: None,
        created_at,
        started_at: Some(created_at),
        finished_at: status.is_terminal().then_some(created_at),
    }
}

#[test]
fn failed_epoch_maps_the_latest_failure_not_a_later_cancellation() {
    let attempts = vec![
        pipeline_attempt("implementation", PipelineMemberAttemptStatus::Failed, 1),
        pipeline_attempt("cleanup", PipelineMemberAttemptStatus::Canceled, 2),
    ];
    let selected = select_epoch_phase_attempt(&attempts, WorkflowStatus::Failed).unwrap();
    assert_eq!(selected.member_key, "implementation");
}

#[test]
fn named_budget_retries_until_its_exhaustion_behavior() {
    let policies = BTreeMap::from([(
        "transient".into(),
        BudgetPolicy {
            attempts: 3,
            exhausted: BudgetExhaustion::Pause,
        },
    )]);
    let mut counters = BTreeMap::new();
    assert_eq!(
        consume_failure_budget(&policies, &mut counters, "transient"),
        Some(FailureBudgetDecision::Retry)
    );
    assert_eq!(
        consume_failure_budget(&policies, &mut counters, "transient"),
        Some(FailureBudgetDecision::Retry)
    );
    assert_eq!(
        consume_failure_budget(&policies, &mut counters, "transient"),
        Some(FailureBudgetDecision::Pause)
    );
    assert_eq!(counters["transient"], 3);
    assert_eq!(
        consume_failure_budget(&policies, &mut counters, "unknown"),
        None
    );
}

// two timestamps in the same 300s window must floor to the identical key, so N-up samplers that read
// slightly different wall clocks still converge to one (org, backend, kind, sampled_at) row.
#[test]
fn timestamps_in_the_same_window_bucket_to_one_key() {
    let interval = Duration::from_secs(300);
    // a window-aligned base (1_700_000_100 is a multiple of 300) so both offsets stay in one window.
    let base = chrono::DateTime::from_timestamp(1_700_000_100, 0).unwrap();
    let a = base + chrono::Duration::seconds(7);
    let b = base + chrono::Duration::seconds(291);
    assert_eq!(
        bucket_to_interval(a, interval),
        bucket_to_interval(b, interval)
    );
    // the bucketed value is the window start and is itself aligned to the interval.
    assert_eq!(bucket_to_interval(a, interval).timestamp() % 300, 0);
}

// adjacent windows must produce distinct keys so successive samples are not collapsed.
#[test]
fn adjacent_windows_bucket_to_distinct_keys() {
    let interval = Duration::from_secs(300);
    let start = chrono::DateTime::from_timestamp(1_700_000_100, 0).unwrap();
    let next = start + chrono::Duration::seconds(300);
    assert_ne!(
        bucket_to_interval(start, interval),
        bucket_to_interval(next, interval)
    );
}

// a zero interval is a degenerate guard: it must not divide-by-zero, just pass the time through.
#[test]
fn zero_interval_passes_through() {
    let now = chrono::DateTime::from_timestamp(1_700_000_123, 0).unwrap();
    assert_eq!(bucket_to_interval(now, Duration::from_secs(0)), now);
}

#[test]
fn workspace_fence_requires_current_version_attempt_and_instance() {
    let workspace_id = Uuid::now_v7();
    let now = chrono::Utc::now();
    let workspace = WorkspaceLease {
        id: workspace_id,
        admission_id: Uuid::now_v7(),
        generation: 2,
        scope: "primary".into(),
        attempt: 3,
        worker_instance_id: "worker-a".into(),
        worker_replica_id: Some(Uuid::now_v7()),
        local_key: "admissions/example/primary/3".into(),
        requirements: Value::Null,
        status: WorkspaceStatus::Active,
        version: 4,
        leased_until: now,
        unavailable_since: None,
        evidence: Value::Null,
        created_at: now,
        updated_at: now,
    };
    let current = WorkspaceAffinity {
        workspace_id,
        worker_instance_id: "worker-a".into(),
        local_key: "admissions/example/primary/3-workspace".into(),
        attempt: 3,
        version: 4,
    };
    assert!(workspace_affinity_matches(&workspace, &current));

    let mut stale = current.clone();
    stale.version = 3;
    assert!(!workspace_affinity_matches(&workspace, &stale));
    stale = current.clone();
    stale.attempt = 2;
    assert!(!workspace_affinity_matches(&workspace, &stale));
    stale = current;
    stale.worker_instance_id = "worker-b".into();
    assert!(!workspace_affinity_matches(&workspace, &stale));

    let mut released = workspace;
    released.status = WorkspaceStatus::Released;
    let released_affinity = released.affinity();
    assert!(!workspace_affinity_matches(&released, &released_affinity));
}
