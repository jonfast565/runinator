//! run records: naming and lookup by open name, and the ancestor path reconstructed from a run
//! snapshot.

use super::*;

#[tokio::test]
async fn workflow_runs_can_be_named_and_fetched_by_open_name() {
    let (db, path) = test_db().await;
    let workflow = save_workflow(&db, &workflow(None, "Ticket Work"))
        .await
        .unwrap();
    let workflow_id = workflow.id.unwrap();
    let open = crate::repository::create_workflow_run(
        &db,
        workflow_id,
        json!({}),
        false,
        Some("Ticket Work: ITP-123".into()),
        Default::default(),
    )
    .await
    .unwrap();
    let terminal = crate::repository::create_workflow_run(
        &db,
        workflow_id,
        json!({}),
        false,
        Some("Ticket Work: ITP-123".into()),
        Default::default(),
    )
    .await
    .unwrap();
    crate::repository::update_workflow_run_status(
        &db,
        terminal.id,
        WorkflowStatus::Succeeded,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    let open_only =
        crate::repository::fetch_workflow_runs_by_name(&db, "Ticket Work: ITP-123".into(), true)
            .await
            .unwrap();

    assert_eq!(open.name.as_deref(), Some("Ticket Work: ITP-123"));
    assert_eq!(
        open_only.iter().map(|run| run.id).collect::<Vec<_>>(),
        vec![open.id]
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn ancestors_in_snapshot_returns_topological_path() {
    let snapshot = WorkflowDefinition {
        id: Some(Uuid::now_v7()),
        name: "ancestors".into(),
        key: None,
        namespace: None,
        org_id: None,
        version: runinator_models::semver::SemVer::new(1, 0, 0),
        enabled: true,
        input_type: runinator_models::types::RuninatorType::Any,
        definition: WorkflowGraph::from_value(json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "a" } } },
                { "id": "a", "kind": "action", "action": { "provider": "console", "function": "run" }, "transitions": { "next": { "$node": "b" } } },
                { "id": "b", "kind": "action", "action": { "provider": "console", "function": "run" }, "transitions": { "next": { "$node": "c" } } },
                { "id": "c", "kind": "action", "action": { "provider": "console", "function": "run" }, "transitions": { "next": { "$node": "end" } } },
                { "id": "end", "kind": "end" }
            ]
        }))
        .unwrap(),
        created_at: None,
        updated_at: None,
    };
    let ancestors = crate::repository::ancestors_in_snapshot(&snapshot, "c").unwrap();
    assert!(ancestors.contains(&"start".to_string()));
    assert!(ancestors.contains(&"a".to_string()));
    assert!(ancestors.contains(&"b".to_string()));
    assert!(!ancestors.contains(&"c".to_string()));
    // start must come before a, a before b.
    let pos_start = ancestors.iter().position(|n| n == "start").unwrap();
    let pos_a = ancestors.iter().position(|n| n == "a").unwrap();
    let pos_b = ancestors.iter().position(|n| n == "b").unwrap();
    assert!(pos_start < pos_a);
    assert!(pos_a < pos_b);
}

#[test]
fn ancestors_in_snapshot_refuses_control_flow_ancestor() {
    let snapshot = WorkflowDefinition {
        id: Some(Uuid::now_v7()),
        name: "loop_ancestor".into(),
        key: None,
        namespace: None,
        org_id: None,
        version: runinator_models::semver::SemVer::new(1, 0, 0),
        enabled: true,
        input_type: runinator_models::types::RuninatorType::Any,
        definition: WorkflowGraph::from_value(json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "loop1" } } },
                { "id": "loop1", "kind": "loop", "parameters": { "items": [], "target": { "$node": "inside" } }, "transitions": { "next": { "$node": "end" } } },
                { "id": "inside", "kind": "action", "action": { "provider": "console", "function": "run" }, "transitions": { "next": { "$node": "loop1" } } },
                { "id": "end", "kind": "end" }
            ]
        }))
        .unwrap(),
        created_at: None,
        updated_at: None,
    };
    let result = crate::repository::ancestors_in_snapshot(&snapshot, "inside");
    assert!(
        result.is_err(),
        "expected refusal for control-flow ancestor"
    );
    let message = result.unwrap_err().to_string();
    assert!(
        message.contains("control-flow") || message.contains("Loop") || message.contains("safely"),
        "error should mention control flow: {message}"
    );
}

#[test]
fn ancestors_in_snapshot_rejects_missing_step() {
    let snapshot = WorkflowDefinition {
        id: Some(Uuid::now_v7()),
        name: "missing".into(),
        key: None,
        namespace: None,
        org_id: None,
        version: runinator_models::semver::SemVer::new(1, 0, 0),
        enabled: true,
        input_type: runinator_models::types::RuninatorType::Any,
        definition: WorkflowGraph::from_value(json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "end" } } },
                { "id": "end", "kind": "end" }
            ]
        }))
        .unwrap(),
        created_at: None,
        updated_at: None,
    };
    let result = crate::repository::ancestors_in_snapshot(&snapshot, "nope");
    assert!(result.is_err());
}
