//! Durable workspace lifecycle phase reporting.
use super::*;

#[test]
fn completed_phases_are_bounded_summary_records() {
    let reporter = WorkspacePhaseReporter::default();
    reporter
        .start("workspace.restore.index")
        .succeeded(runinator_models::json!({"objects": 52_000, "packs": 1}));

    let events = reporter.drain();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].phase, "workspace.restore.index");
    assert_eq!(events[0].status, "succeeded");
    assert_eq!(events[0].details["objects"], 52_000);
    assert!(reporter.drain().is_empty());
}

#[test]
fn an_unfinished_phase_records_one_failure() {
    let reporter = WorkspacePhaseReporter::default();
    drop(reporter.start("workspace.snapshot.capture"));

    let events = reporter.drain();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].status, "failed");
}
