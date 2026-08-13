//! workflow runs: listing, claiming under a lease, correlation-key routing for waiting runs, and the
//! cascade a workflow delete performs over its runs and execution records.

use super::*;

#[tokio::test]
async fn fetch_recent_workflow_runs_returns_all_workflows_newest_first() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-runs-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let first = db
        .upsert_workflow(&workflow("first"))
        .await
        .unwrap()
        .id
        .unwrap();
    let second = db
        .upsert_workflow(&workflow("second"))
        .await
        .unwrap()
        .id
        .unwrap();
    let first_snapshot = db.fetch_workflow(first).await.unwrap().unwrap();
    let second_snapshot = db.fetch_workflow(second).await.unwrap().unwrap();
    let older = db
        .create_workflow_run(
            first,
            first_snapshot,
            runinator_models::json!({}),
            runinator_models::json!({}),
            None,
            Default::default(),
        )
        .await
        .unwrap();
    let newer = db
        .create_workflow_run(
            second,
            second_snapshot,
            runinator_models::json!({}),
            runinator_models::json!({}),
            None,
            Default::default(),
        )
        .await
        .unwrap();

    let runs = db.fetch_recent_workflow_runs(100).await.unwrap();
    assert_eq!(
        runs.iter().map(|run| run.id).collect::<Vec<_>>(),
        vec![newer.id, older.id]
    );
    assert_eq!(
        runs.iter().map(|run| run.workflow_id).collect::<Vec<_>>(),
        vec![second, first]
    );
    assert_eq!(
        runs[0]
            .workflow_snapshot
            .as_ref()
            .map(|workflow| workflow.name.as_str()),
        Some("second")
    );

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn workflow_execution_state_round_trips_through_relational_projection() {
    let path = std::env::temp_dir().join(format!(
        "runinator-execution-state-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let workflow = db
        .upsert_workflow(&workflow("execution-state"))
        .await
        .unwrap();
    let raw = runinator_models::json!({
        "control": { "pause_requested": true },
        "debug": { "enabled": true, "mode": "breakpoints", "breakpoints": ["review"] },
        "watch_fired": true,
        "event_sources": { "hook": { "pending_event": { "id": 7 } } },
        "pending_interrupts": [{
            "id": Uuid::now_v7(), "source": "external", "payload": { "why": "test" },
            "requested_at": "2026-08-13T19:00:00Z"
        }],
        "cursors": [{
            "id": Uuid::now_v7(), "node_id": "review", "forked_by": "parallel",
            "loops": [{ "node_id": "each", "index": 2, "items": [1, 2, 3], "results": ["a"] }],
            "try": { "node_id": "attempt", "phase": "catch" },
            "debug": { "paused": true, "current_node_id": "review", "context_json": { "x": 1 } },
            "handled": ["wake:00000000-0000-0000-0000-000000000000:1"]
        }]
    });
    let expected = runinator_models::workflow_state::WorkflowExecutionState::from_state(&raw);
    let run = db
        .create_workflow_run(
            workflow.id.unwrap(),
            workflow,
            runinator_models::json!({}),
            raw,
            None,
            Default::default(),
        )
        .await
        .unwrap();

    let fetched = db.fetch_workflow_run(run.id).await.unwrap().unwrap();
    assert_eq!(fetched.execution_state.to_state(), expected.to_state());
    assert_eq!(fetched.state, runinator_models::json!({}));
    let cursor_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM workflow_run_cursors WHERE workflow_run_id = ?")
            .bind(run.id)
            .fetch_one(db.pool())
            .await
            .unwrap();
    let frame_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM workflow_cursor_frames WHERE workflow_run_id = ?")
            .bind(run.id)
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(cursor_count, 1);
    assert!(frame_count >= 4);

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn legacy_workflow_state_is_backfilled_and_cleared() {
    let path = std::env::temp_dir().join(format!(
        "runinator-execution-state-migration-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let workflow = db.upsert_workflow(&workflow("legacy-state")).await.unwrap();
    let run = db
        .create_workflow_run(
            workflow.id.unwrap(),
            workflow,
            runinator_models::json!({}),
            runinator_models::json!({}),
            None,
            Default::default(),
        )
        .await
        .unwrap();
    sqlx::query("DELETE FROM workflow_run_execution_states WHERE workflow_run_id = ?")
        .bind(run.id)
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE workflow_runs SET state = ? WHERE id = ?")
        .bind(r#"{"watch_fired":true,"event_source_hook":{"pending_event":{"ok":true}}}"#)
        .bind(run.id)
        .execute(db.pool())
        .await
        .unwrap();

    db.migrate_workflow_execution_states().await.unwrap();
    let migrated = db.fetch_workflow_run(run.id).await.unwrap().unwrap();
    assert!(migrated.execution_state.watch_fired);
    assert!(migrated.execution_state.event_source("hook").is_some());
    assert_eq!(migrated.state, runinator_models::json!({}));

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn workflow_runs_can_be_created_and_queried_by_open_name() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-runs-by-name-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let workflow_id = db
        .upsert_workflow(&workflow("ticket work"))
        .await
        .unwrap()
        .id
        .unwrap();
    let snapshot = db.fetch_workflow(workflow_id).await.unwrap().unwrap();
    let open = db
        .create_workflow_run(
            workflow_id,
            snapshot.clone(),
            runinator_models::json!({}),
            runinator_models::json!({}),
            Some("Ticket Work: ITP-123".into()),
            Default::default(),
        )
        .await
        .unwrap();
    let terminal = db
        .create_workflow_run(
            workflow_id,
            snapshot,
            runinator_models::json!({}),
            runinator_models::json!({}),
            Some("Ticket Work: ITP-123".into()),
            Default::default(),
        )
        .await
        .unwrap();
    db.update_workflow_run_status(terminal.id, WorkflowStatus::Succeeded, None, None, None)
        .await
        .unwrap();

    let all = db
        .fetch_workflow_runs_by_name("Ticket Work: ITP-123".into(), false)
        .await
        .unwrap();
    let open_only = db
        .fetch_workflow_runs_by_name("Ticket Work: ITP-123".into(), true)
        .await
        .unwrap();

    assert_eq!(
        all.iter().map(|run| run.id).collect::<Vec<_>>(),
        vec![terminal.id, open.id]
    );
    assert_eq!(
        open_only.iter().map(|run| run.id).collect::<Vec<_>>(),
        vec![open.id]
    );
    assert_eq!(open.name.as_deref(), Some("Ticket Work: ITP-123"));

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn scheduler_claims_open_workflow_runs_once_until_lease_expires() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-claims-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let workflow_id = db
        .upsert_workflow(&workflow("claim-test"))
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
    let now = Utc::now();

    let first = db
        .claim_workflow_runs_for_scheduler(
            "scheduler-a".into(),
            vec![WorkflowStatus::Queued],
            now,
            now + Duration::seconds(60),
            10,
        )
        .await
        .unwrap();
    let second = db
        .claim_workflow_runs_for_scheduler(
            "scheduler-b".into(),
            vec![WorkflowStatus::Queued],
            now,
            now + Duration::seconds(60),
            10,
        )
        .await
        .unwrap();
    let expired = db
        .claim_workflow_runs_for_scheduler(
            "scheduler-b".into(),
            vec![WorkflowStatus::Queued],
            now + Duration::seconds(61),
            now + Duration::seconds(120),
            10,
        )
        .await
        .unwrap();

    assert_eq!(
        first.iter().map(|run| run.id).collect::<Vec<_>>(),
        vec![run.id]
    );
    assert!(second.is_empty());
    assert_eq!(
        expired.iter().map(|run| run.id).collect::<Vec<_>>(),
        vec![run.id]
    );

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn delete_workflow_cascades_runs_and_execution_records() {
    let path = std::env::temp_dir().join(format!(
        "runinator-delete-cascade-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let workflow_id = db
        .upsert_workflow(&workflow("cascade-test"))
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
    let node_run = db
        .create_workflow_node_run(
            run.id,
            "node-a".into(),
            runinator_models::json!({}),
            None,
            None,
        )
        .await
        .unwrap();
    // a chunk result event populates workflow_node_chunks + workflow_result_events.
    let command = action_command(run.id, node_run.id, &node_run.node_id);
    let chunk = WorkflowResultEvent::chunk(
        &command,
        NewRunChunk {
            stream: "log".into(),
            content: "hello".into(),
        },
    );
    db.apply_workflow_result_event(&chunk).await.unwrap();
    // a ready node populates workflow_orchestration_events + workflow_ready_nodes.
    let event = runinator_models::orchestration::NewOrchestrationEvent::new(
        run.id,
        Some("start".into()),
        "workflow_run_created",
        runinator_models::json!({}),
    );
    db.enqueue_ready_node(event, "start".into(), Utc::now())
        .await
        .unwrap();

    db.delete_workflow(workflow_id).await.unwrap();

    assert!(db.fetch_workflow(workflow_id).await.unwrap().is_none());
    assert!(db.fetch_recent_workflow_runs(100).await.unwrap().is_empty());
    assert!(
        db.fetch_workflow_node_run(node_run.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.fetch_workflow_node_run_chunks(node_run.id, None, 100)
            .await
            .unwrap()
            .is_empty()
    );

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn waiting_signal_runs_are_routable_by_correlation_key() {
    let path = std::env::temp_dir().join(format!(
        "runinator-signal-correlation-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let workflow_id = db
        .upsert_workflow(&workflow("signal-test"))
        .await
        .unwrap()
        .id
        .unwrap();
    let snapshot = db.fetch_workflow(workflow_id).await.unwrap().unwrap();

    // two runs park a signal node on the same name but different correlation keys.
    let park = |key: &'static str| {
        let db = &db;
        let snapshot = snapshot.clone();
        async move {
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
            let node_run = db
                .create_workflow_node_run(
                    run.id,
                    "wait_review".into(),
                    runinator_models::json!({}),
                    None,
                    None,
                )
                .await
                .unwrap();
            let state = runinator_models::workflow_state::SignalState {
                name: "github.review".into(),
                correlation_key: Some(key.into()),
            };
            db.update_workflow_node_run(
                node_run.id,
                WorkflowStatus::Waiting,
                Some(1),
                None,
                None,
                Some(serde_json::to_value(&state).unwrap().into()),
                Some("signal_waiting".into()),
                None,
            )
            .await
            .unwrap();
            (run.id, node_run.id)
        }
    };
    let (run_abc, node_abc) = park("ABC-1").await;
    let (_run_xyz, _node_xyz) = park("XYZ-9").await;

    let waiting = db
        .fetch_workflow_node_runs_by_status(WorkflowStatus::Waiting)
        .await
        .unwrap();
    assert_eq!(waiting.len(), 2);

    // the same predicate the ws repository uses must select exactly the matching run.
    let matched: Vec<_> = waiting
        .iter()
        .filter(|run| {
            serde_json::from_value::<runinator_models::workflow_state::SignalState>(
                run.state.clone().into(),
            )
            .map(|state| {
                state.name == "github.review" && state.correlation_key.as_deref() == Some("ABC-1")
            })
            .unwrap_or(false)
        })
        .collect();
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].id, node_abc);
    assert_eq!(matched[0].workflow_run_id, run_abc);

    let _ = fs::remove_file(path);
}
