use super::action::{
    TargetResolution, default_foreign_language_runtime, foreign_language_runtime,
    has_dedicated_workers, replica_labels_match, target_for, target_for_labels,
};
use super::assert::evaluate_assertions;
use super::await_run::parse_await_mode;
use super::barrier::arrivals_complete;
use super::circuit_breaker::is_circuit_open;
use super::collect::threshold_reached;
use super::debounce::deadline_elapsed;
use super::engine::reentry_exhausted;
use super::event_source::event_type_matches;
use super::mutex::{holder_run_id, record_is_held_by_other};
use super::throttle::bucket_has_tokens;
use super::transform::resolve_bindings;
use super::transitions::{timed_out, timed_out_since_created};
use runinator_comm::ActionTarget;
use runinator_models::{
    value::Value,
    workflows::{WorkflowNode, WorkflowNodeRun},
};
use uuid::Uuid;

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
fn general_pool_actions_target_any() {
    let replica = Uuid::now_v7();
    // a non-local provider always goes to the general pool, regardless of the launching replica.
    assert_eq!(
        target_for("console", Some(replica), true),
        TargetResolution::Ready(ActionTarget::Any)
    );
    assert_eq!(
        target_for("console", None, false),
        TargetResolution::Ready(ActionTarget::Any)
    );
}

#[test]
fn local_actions_pin_to_a_live_launching_replica_else_park() {
    let replica = Uuid::now_v7();
    // bound desktop is connected: pin the action to that exact replica.
    assert_eq!(
        target_for("local", Some(replica), true),
        TargetResolution::Ready(ActionTarget::Replica {
            replica_id: replica
        })
    );
    // bound desktop is offline: park rather than publish into a queue no one drains.
    assert_eq!(
        target_for("local", Some(replica), false),
        TargetResolution::Park
    );
    // no launching replica recorded: nowhere local to run, so park.
    assert_eq!(target_for("local", None, true), TargetResolution::Park);
}

#[test]
fn label_targeted_actions_route_to_a_matching_worker_else_park() {
    let mut selector = std::collections::BTreeMap::new();
    selector.insert("runner".to_string(), "creds-sync".to_string());
    // a live worker carries the label: dispatch a labelled target.
    assert_eq!(
        target_for_labels(&selector, true),
        TargetResolution::Ready(ActionTarget::Labels {
            selector: selector.clone()
        })
    );
    // no matching worker connected: park (the node timeout later fails the run).
    assert_eq!(target_for_labels(&selector, false), TargetResolution::Park);
}

#[test]
fn org_is_dedicated_only_when_it_has_a_worker_allocation() {
    use runinator_models::billing::OrgResourceGroup;
    use runinator_models::provisioning::ProvisionBackend;
    use runinator_models::replicas::ReplicaKind;
    use uuid::Uuid;

    let org_id = Uuid::now_v7();
    let worker = |desired| OrgResourceGroup {
        org_id,
        backend: ProvisionBackend::Supervisor,
        kind: ReplicaKind::Worker,
        desired,
        dedicated: true,
    };

    // no allocation, or a zeroed one, leaves the org on the shared pool (no org label injected).
    assert!(!has_dedicated_workers(&[]));
    assert!(!has_dedicated_workers(&[worker(0)]));
    // a live worker allocation opts the org into dedicated routing.
    assert!(has_dedicated_workers(&[worker(2)]));
    // a waker-only allocation does not make workers dedicated.
    assert!(!has_dedicated_workers(&[OrgResourceGroup {
        org_id,
        backend: ProvisionBackend::Supervisor,
        kind: ReplicaKind::Waker,
        desired: 3,
        dedicated: true,
    }]));
}

#[test]
fn replica_labels_match_requires_superset() {
    let mut required = std::collections::BTreeMap::new();
    required.insert("runner".to_string(), "creds-sync".to_string());

    // exact and superset label sets match.
    let exact = runinator_models::json!({ "labels": { "runner": "creds-sync" } });
    assert!(replica_labels_match(&exact, &required));
    let superset =
        runinator_models::json!({ "labels": { "runner": "creds-sync", "zone": "onprem" } });
    assert!(replica_labels_match(&superset, &required));

    // wrong value, missing key, and absent labels object all fail to match.
    let wrong = runinator_models::json!({ "labels": { "runner": "other" } });
    assert!(!replica_labels_match(&wrong, &required));
    let missing = runinator_models::json!({ "labels": { "zone": "onprem" } });
    assert!(!replica_labels_match(&missing, &required));
    let unlabeled = runinator_models::json!({ "broker_backend": "tcp" });
    assert!(!replica_labels_match(&unlabeled, &required));
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

// --- new node type unit tests ---

#[test]
fn assert_node_returns_violations_for_failing_conditions() {
    // `{"all": []}` is vacuously true; `{"any": []}` is vacuously false.
    let params = serde_json::from_str::<Value>(
        r#"{
        "assertions": [
            { "name": "always_true",  "condition": {"all": []}, "message": "should not appear" },
            { "name": "always_false", "condition": {"any": []}, "message": "invariant violated" }
        ]
    }"#,
    )
    .unwrap()
    .into();
    let context = serde_json::from_str::<Value>("{}").unwrap().into();
    let violations = evaluate_assertions(&params, &context);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].name, "always_false");
    assert_eq!(violations[0].message, "invariant violated");
}

#[test]
fn transform_node_resolves_literal_bindings() {
    let params = serde_json::from_str::<Value>(r#"{ "bindings": { "x": 42, "label": "hello" } }"#)
        .unwrap()
        .into();
    let context = serde_json::from_str::<Value>("{}").unwrap().into();
    let result = resolve_bindings(&params, &context);
    assert_eq!(result.get("x").and_then(|v| v.as_i64()), Some(42));
    assert_eq!(result.get("label").and_then(|v| v.as_str()), Some("hello"));
}

#[test]
fn audit_node_build_record_includes_required_fields() {
    use super::audit::build_audit_record;
    let run_id = Uuid::now_v7();
    let resolved = serde_json::from_str::<Value>(
        r#"{ "actor": "alice", "action": "approved", "target": "pr-42" }"#,
    )
    .unwrap()
    .into();
    let record = build_audit_record(run_id, "my_audit", &resolved);
    assert_eq!(
        record.get("action").and_then(|v| v.as_str()),
        Some("approved")
    );
    assert_eq!(record.get("actor").and_then(|v| v.as_str()), Some("alice"));
    assert_eq!(
        record.get("node_id").and_then(|v| v.as_str()),
        Some("my_audit")
    );
}

#[test]
fn checkpoint_node_parses_name_from_params() {
    use super::checkpoint::parse_checkpoint_name;
    let with_name = serde_json::from_str::<Value>(r#"{ "name": "after-ingest" }"#)
        .unwrap()
        .into();
    assert_eq!(parse_checkpoint_name(&with_name, "node1"), "after-ingest");
    let without_name = serde_json::from_str::<Value>("{}").unwrap().into();
    assert_eq!(parse_checkpoint_name(&without_name, "node1"), "node1");
}

#[test]
fn mutex_record_is_held_by_other_respects_released_flag() {
    let run_a = Uuid::now_v7();
    let run_b = Uuid::now_v7();
    let held = serde_json::from_str::<Value>(&format!(r#"{{ "held_by_run_id": "{run_a}" }}"#))
        .unwrap()
        .into();
    // held by run_a, checking from run_b → held by other.
    assert!(record_is_held_by_other(&held, run_b));
    // held by run_a, checking from run_a itself → not held by other.
    assert!(!record_is_held_by_other(&held, run_a));
    let released = serde_json::from_str::<Value>(&format!(
        r#"{{ "held_by_run_id": "{run_a}", "released_at": 1 }}"#
    ))
    .unwrap()
    .into();
    // released records are never considered held.
    assert!(!record_is_held_by_other(&released, run_b));
}

#[test]
fn mutex_holder_run_id_parses_only_valid_uuids() {
    let run = Uuid::now_v7();
    let held = serde_json::from_str::<Value>(&format!(r#"{{ "held_by_run_id": "{run}" }}"#))
        .unwrap()
        .into();
    assert_eq!(holder_run_id(&held), Some(run));
    // a record with no holder, or a malformed id, resolves to no holder.
    let empty = serde_json::from_str::<Value>("{}").unwrap().into();
    assert_eq!(holder_run_id(&empty), None);
    let malformed = serde_json::from_str::<Value>(r#"{ "held_by_run_id": "not-a-uuid" }"#)
        .unwrap()
        .into();
    assert_eq!(holder_run_id(&malformed), None);
}

#[test]
fn throttle_bucket_has_tokens_resets_on_expired_window() {
    let now = chrono::Utc::now().timestamp();
    // window started 120s ago; max 5/60s → window expired, always has tokens.
    let expired = serde_json::from_str::<Value>(&format!(
        r#"{{ "window_start": {}, "tokens_used": 5 }}"#,
        now - 120
    ))
    .unwrap()
    .into();
    assert!(bucket_has_tokens(&expired, 5, 60));
    // window is fresh and tokens exhausted → no tokens.
    let full = serde_json::from_str::<Value>(&format!(
        r#"{{ "window_start": {}, "tokens_used": 5 }}"#,
        now
    ))
    .unwrap()
    .into();
    assert!(!bucket_has_tokens(&full, 5, 60));
    // window is fresh but capacity remains → has tokens.
    let partial = serde_json::from_str::<Value>(&format!(
        r#"{{ "window_start": {}, "tokens_used": 3 }}"#,
        now
    ))
    .unwrap()
    .into();
    assert!(bucket_has_tokens(&partial, 5, 60));
}

#[test]
fn await_run_node_defaults_to_all_mode() {
    let params_all = serde_json::from_str::<Value>(r#"{ "mode": "all" }"#)
        .unwrap()
        .into();
    let params_any = serde_json::from_str::<Value>(r#"{ "mode": "any" }"#)
        .unwrap()
        .into();
    let params_missing = serde_json::from_str::<Value>("{}").unwrap().into();
    assert_eq!(parse_await_mode(&params_all), "all");
    assert_eq!(parse_await_mode(&params_any), "any");
    assert_eq!(parse_await_mode(&params_missing), "all");
}

#[test]
fn debounce_node_detects_elapsed_deadline() {
    let past = chrono::Utc::now().timestamp() - 10;
    let future = chrono::Utc::now().timestamp() + 60;
    assert!(deadline_elapsed(past));
    assert!(!deadline_elapsed(future));
}

#[test]
fn collect_node_threshold_detection() {
    assert!(threshold_reached(5, 5));
    assert!(threshold_reached(10, 5));
    assert!(!threshold_reached(4, 5));
    // threshold 0 means no threshold; never triggered by count alone.
    assert!(!threshold_reached(100, 0));
}

#[test]
fn barrier_node_arrivals_complete() {
    assert!(arrivals_complete(3, 3));
    assert!(arrivals_complete(5, 3));
    assert!(!arrivals_complete(2, 3));
    // expected 0 is degenerate; never complete.
    assert!(!arrivals_complete(0, 0));
}

#[test]
fn circuit_breaker_open_respects_cooldown() {
    let now = chrono::Utc::now().timestamp();
    // tripped recently → open during cooldown.
    let open = serde_json::from_str::<Value>(&format!(
        r#"{{ "circuit_state": "open", "last_tripped_at": {} }}"#,
        now - 30
    ))
    .unwrap()
    .into();
    assert!(is_circuit_open(&open, 120, now));
    // tripped long ago → cooldown elapsed → not open.
    let recovered = serde_json::from_str::<Value>(&format!(
        r#"{{ "circuit_state": "open", "last_tripped_at": {} }}"#,
        now - 300
    ))
    .unwrap()
    .into();
    assert!(!is_circuit_open(&recovered, 120, now));
    // closed state → never open regardless.
    let closed =
        serde_json::from_str::<Value>(r#"{ "circuit_state": "closed", "last_tripped_at": 0 }"#)
            .unwrap()
            .into();
    assert!(!is_circuit_open(&closed, 120, now));
}

#[test]
fn event_source_type_matching() {
    let event = serde_json::from_str::<Value>(r#"{ "type": "file.uploaded" }"#)
        .unwrap()
        .into();
    assert!(event_type_matches(&event, "file.uploaded"));
    assert!(!event_type_matches(&event, "user.created"));
    // wildcard matches everything.
    assert!(event_type_matches(&event, "*"));
}

#[test]
fn timed_out_since_created_catches_a_run_that_never_reached_running() {
    let node: WorkflowNode = serde_json::from_value(serde_json::json!({
        "id": "wait",
        "kind": "signal",
        "timeout_seconds": 60,
    }))
    .expect("node");
    // a parked run (signal/approval/input/action-park/etc.) never transitions through `Running`,
    // so the db layer never populates `started_at`. the deadline-from-dispatch check must therefore
    // be blind to a stale park, while the deadline-from-creation check must still catch it.
    let run = node_run("wait", "waiting");
    assert!(!timed_out(&node, &run));
    assert!(timed_out_since_created(&node, &run));
}
