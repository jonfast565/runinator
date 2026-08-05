//! reconstructing a run edge history from the explicit origins recorded on each node run.

use super::*;

#[tokio::test]
async fn transitions_reconstruct_from_explicit_origins() {
    let path = std::env::temp_dir().join(format!(
        "runinator-transitions-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let workflow_id = db
        .upsert_workflow(&workflow("transitions-test"))
        .await
        .unwrap()
        .id
        .unwrap();
    let snapshot = db.fetch_workflow(workflow_id).await.unwrap().unwrap();
    let run = db
        .create_workflow_run(
            workflow_id,
            snapshot,
            runinator_models::json!({}),
            runinator_models::json!({}),
            None,
            Default::default(),
        )
        .await
        .unwrap();

    // start -> parallel, then parallel fans out to two branches that both point back at the parent.
    let start = db
        .create_workflow_node_run(run.id, "start".into(), runinator_models::json!({}), None)
        .await
        .unwrap();
    db.update_workflow_node_run(
        start.id,
        WorkflowStatus::Succeeded,
        None,
        None,
        None,
        None,
        Some("succeeded".into()),
        None,
    )
    .await
    .unwrap();
    let parent = db
        .create_workflow_node_run(
            run.id,
            "fork".into(),
            runinator_models::json!({}),
            Some(start.id),
        )
        .await
        .unwrap();
    let branch_a = db
        .create_workflow_node_run(
            run.id,
            "a".into(),
            runinator_models::json!({}),
            Some(parent.id),
        )
        .await
        .unwrap();
    let branch_b = db
        .create_workflow_node_run(
            run.id,
            "b".into(),
            runinator_models::json!({}),
            Some(parent.id),
        )
        .await
        .unwrap();

    let transitions = db.fetch_run_transitions(run.id).await.unwrap();
    let edges: std::collections::HashSet<(Option<String>, String)> = transitions
        .iter()
        .map(|t| (t.from_node.clone(), t.to_node.clone()))
        .collect();
    assert!(edges.contains(&(None, "start".to_string())));
    assert!(edges.contains(&(Some("start".to_string()), "fork".to_string())));
    // both branches point at the fork parent, not chained by insertion order.
    assert!(edges.contains(&(Some("fork".to_string()), "a".to_string())));
    assert!(edges.contains(&(Some("fork".to_string()), "b".to_string())));
    let _ = (branch_a, branch_b);

    let stats = db
        .fetch_node_transition_stats(workflow_id, Some("fork".into()))
        .await
        .unwrap();
    let mut targets: Vec<String> = stats.iter().map(|s| s.to_node.clone()).collect();
    targets.sort();
    assert_eq!(targets, vec!["a".to_string(), "b".to_string()]);
    assert!(stats.iter().all(|s| s.count == 1 && s.from_node == "fork"));

    let _ = fs::remove_file(path);
}
