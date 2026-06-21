use super::action::{default_foreign_language_runtime, foreign_language_runtime};
use super::engine::reentry_exhausted;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeRun};

fn reentry_node(max_visits: i64, on_exhausted: bool) -> WorkflowNode {
    let mut reentry = serde_json::json!({ "enabled": true, "max_visits": max_visits });
    if on_exhausted {
        reentry["on_exhausted"] = serde_json::json!({ "$node": "after" });
    }
    serde_json::from_value(serde_json::json!({
        "id": "loop",
        "kind": "condition",
        "reentry": reentry,
    }))
    .expect("reentry node")
}

fn node_run(node_id: &str, status: &str) -> WorkflowNodeRun {
    serde_json::from_value(serde_json::json!({
        "id": uuid::Uuid::now_v7(),
        "workflow_run_id": uuid::Uuid::now_v7(),
        "node_id": node_id,
        "status": status,
        "attempt": 1,
        "parameters": null,
        "output_json": null,
        "state": null,
        "transition_reason": null,
        "created_at": "2026-01-01T00:00:00Z",
        "started_at": null,
        "finished_at": null,
        "message": null,
    }))
    .expect("node run")
}

#[test]
fn reentry_exhausted_fires_only_at_the_visit_cap() {
    let node = reentry_node(2, true);
    // one completed visit is under the cap; the latest is terminal so the next entry is fresh.
    let runs = vec![node_run("loop", "succeeded")];
    assert!(!reentry_exhausted(&node, &runs, runs.last()));
    // a second completed visit reaches the cap, so a fresh re-entry exits via on_exhausted.
    let runs = vec![node_run("loop", "succeeded"), node_run("loop", "succeeded")];
    assert!(reentry_exhausted(&node, &runs, runs.last()));
}

#[test]
fn reentry_exhausted_ignores_in_flight_and_unbounded_nodes() {
    let node = reentry_node(2, true);
    // the cap is reached but the latest visit is still running, so we never abandon it mid-flight.
    let runs = vec![node_run("loop", "succeeded"), node_run("loop", "running")];
    assert!(!reentry_exhausted(&node, &runs, runs.last()));
    // a node without reentry enabled is never bounded by this guard.
    let plain: WorkflowNode = serde_json::from_value(serde_json::json!({
        "id": "loop",
        "kind": "condition",
    }))
    .expect("plain node");
    let runs = vec![node_run("loop", "succeeded"), node_run("loop", "succeeded")];
    assert!(!reentry_exhausted(&plain, &runs, runs.last()));
}

#[test]
fn foreign_language_runtime_canonicalizes_supported_aliases() {
    let cases = [
        ("python", "python", "python:3.12"),
        ("py", "python", "python:3.12"),
        ("javascript", "javascript", "node:22"),
        ("js", "javascript", "node:22"),
        ("node", "javascript", "node:22"),
        ("bash", "bash", "bash:5.2"),
        ("sh", "bash", "bash:5.2"),
        ("ruby", "ruby", "ruby:3.3"),
        ("rb", "ruby", "ruby:3.3"),
        ("perl", "perl", "perl:5.40"),
        ("pl", "perl", "perl:5.40"),
        ("php", "php", "php:8.3-cli"),
    ];

    for (input, canonical, image) in cases {
        assert_eq!(foreign_language_runtime(input), Some((canonical, image)));
    }
    assert_eq!(foreign_language_runtime("lua"), None);
}

#[test]
fn default_foreign_language_runtime_has_image_and_empty_setup_script() {
    let runtime = default_foreign_language_runtime("node:22");

    assert_eq!(
        runtime.pointer("/image").and_then(|value| value.as_str()),
        Some("node:22")
    );
    assert_eq!(
        runtime
            .pointer("/setup_script")
            .and_then(|value| value.as_str()),
        Some("")
    );
}
