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

#[test]
fn atomic_dashboard_snapshot_can_replace_an_existing_file() {
    let directory =
        std::env::temp_dir().join(format!("runinator-dashboard-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("state.json");
    let tracker = StateTracker::new();
    tracker.starting("pack-import", "startup-hook");
    tracker.write(&path).unwrap();
    tracker.failed("pack-import", "missing nested asset");
    tracker.write(&path).unwrap();

    let dashboard: LocalDashboardSnapshot =
        serde_json::from_slice(&std::fs::read(directory.join("dashboard.json")).unwrap()).unwrap();
    assert_eq!(dashboard.version, 1);
    assert_eq!(dashboard.components[0].status, "failed");
    assert_eq!(
        dashboard.components[0].last_error.as_deref(),
        Some("missing nested asset")
    );
    assert!(std::path::Path::new(&dashboard.log_location).is_absolute());

    std::fs::remove_dir_all(directory).unwrap();
}
