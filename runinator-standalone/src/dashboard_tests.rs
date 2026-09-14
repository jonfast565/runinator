//! aggregate dashboard summaries for the standalone composition host.

use super::*;
use runinator_models::local_runtime::{
    LocalRuntimeComponentSnapshot, LocalRuntimeHostKind, LocalRuntimeSnapshot,
};

fn component(kind: &str, status: &str, restarts: u32) -> LocalRuntimeComponentSnapshot {
    LocalRuntimeComponentSnapshot {
        id: format!("{kind}-{status}"),
        kind: kind.into(),
        status: status.into(),
        restarts,
        uptime_seconds: None,
        last_error: None,
    }
}

#[test]
fn summaries_aggregate_roles_and_exclude_stopped_nodes() {
    let snapshot = LocalRuntimeSnapshot {
        host_kind: LocalRuntimeHostKind::Standalone,
        pid: 42,
        started_at: String::new(),
        updated_at: String::new(),
        components: vec![
            component("worker", "running", 1),
            component("worker", "backoff", 2),
            component("worker", "stopped", 9),
            component("background", "starting", 0),
            component("waker", "failed", 3),
            component("startup-hook", "running", 0),
        ],
    };

    let summaries = summarize(&snapshot);
    assert_eq!(
        summaries[WORKER],
        RoleSummary {
            configured: 2,
            running: 1,
            degraded: 1,
            restarts: 3,
            ..RoleSummary::default()
        }
    );
    assert_eq!(summaries[ENGINE].starting, 1);
    assert_eq!(summaries[WAKER].degraded, 1);
    assert_eq!(summaries[STANDALONE].configured, 5);
    assert_eq!(summaries[STANDALONE].restarts, 6);
}

#[test]
fn role_mapping_uses_existing_metric_component_names() {
    assert_eq!(component_name("webservice"), WEB_SERVICE);
    assert_eq!(component_name("background"), ENGINE);
    assert_eq!(component_name("worker"), WORKER);
    assert_eq!(component_name("waker"), WAKER);
    assert_eq!(component_name("startup-hook"), STANDALONE);
}

#[test]
fn summaries_include_zero_count_roles() {
    let snapshot = LocalRuntimeSnapshot {
        host_kind: LocalRuntimeHostKind::Standalone,
        pid: 42,
        started_at: String::new(),
        updated_at: String::new(),
        components: Vec::new(),
    };

    let summaries = summarize(&snapshot);
    assert_eq!(summaries.len(), 8);
    assert_eq!(summaries[ENGINE], RoleSummary::default());
    assert_eq!(summaries[WORKER], RoleSummary::default());
    assert_eq!(summaries[WAKER], RoleSummary::default());
}
