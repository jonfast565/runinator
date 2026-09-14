use super::*;

#[test]
fn snapshot_tracks_logical_components() {
    let tracker = StateTracker::new();
    tracker.starting("worker-1", "worker");
    tracker.running("worker-1");

    let snapshot = tracker.snapshot();
    assert_eq!(snapshot.host_kind, LocalRuntimeHostKind::Standalone);
    assert_eq!(snapshot.components.len(), 1);
    assert_eq!(snapshot.components[0].id, "worker-1");
    assert_eq!(snapshot.components[0].status, "running");

    tracker.restarting("worker-1");
    tracker.starting("worker-1", "worker");
    tracker.running("worker-1");
    assert_eq!(tracker.snapshot().components[0].restarts, 1);
}
