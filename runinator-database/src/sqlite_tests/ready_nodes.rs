//! the ready-node queue: claim leases, announce windows, generation collapsing on re-enqueue, and the
//! settlement a terminal run performs over its pending rows.

use super::*;

#[tokio::test]
async fn ready_nodes_are_claimed_once_until_lease_expires() {
    let path = std::env::temp_dir().join(format!(
        "runinator-ready-nodes-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let workflow_id = db
        .upsert_workflow(&workflow("ready-node-test"))
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
    let event = runinator_models::orchestration::NewOrchestrationEvent::new(
        run.id,
        Some("start".into()),
        "workflow_run_created",
        runinator_models::json!({}),
    );
    let ready = db
        .enqueue_ready_node(event, "start".into(), Utc::now())
        .await
        .unwrap()
        .expect("ready node should be inserted");

    let first = db
        .claim_ready_nodes(
            "scheduler-a".into(),
            Utc::now(),
            Utc::now() + Duration::seconds(30),
            10,
        )
        .await
        .unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].id, ready.id);

    let second = db
        .claim_ready_nodes(
            "scheduler-b".into(),
            Utc::now(),
            Utc::now() + Duration::seconds(30),
            10,
        )
        .await
        .unwrap();
    assert!(second.is_empty());

    let reclaimed = db
        .claim_ready_nodes(
            "scheduler-b".into(),
            Utc::now() + Duration::seconds(31),
            Utc::now() + Duration::seconds(60),
            10,
        )
        .await
        .unwrap();
    assert_eq!(reclaimed.len(), 1);
    assert_eq!(reclaimed[0].claimed_by.as_deref(), Some("scheduler-b"));

    assert!(
        db.complete_ready_node(ready.id, "scheduler-b".into())
            .await
            .unwrap()
    );

    let after_complete = db
        .claim_ready_nodes(
            "scheduler-a".into(),
            Utc::now() + Duration::seconds(61),
            Utc::now() + Duration::seconds(90),
            10,
        )
        .await
        .unwrap();
    assert!(after_complete.is_empty());

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn terminal_run_status_settles_its_pending_ready_nodes() {
    let path = std::env::temp_dir().join(format!(
        "runinator-terminal-ready-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let workflow_id = db
        .upsert_workflow(&workflow("terminal-ready-test"))
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
    // pending ready nodes across two different nodes, mimicking work left mid-flight.
    for node in ["sync_a", "sync_b"] {
        let event = runinator_models::orchestration::NewOrchestrationEvent::new(
            run.id,
            Some(node.into()),
            "local_target_poll",
            runinator_models::json!({}),
        );
        db.enqueue_ready_node(event, node.into(), Utc::now())
            .await
            .unwrap()
            .expect("ready node should be inserted");
    }
    assert_eq!(
        db.fetch_pending_ready_nodes(Utc::now(), 100)
            .await
            .unwrap()
            .len(),
        2
    );

    // finalizing the run to a terminal state must settle every pending ready node it owns, so the
    // wake publisher stops rescanning a dead run.
    db.update_workflow_run_status(run.id, WorkflowStatus::Succeeded, None, None, None)
        .await
        .unwrap();
    assert!(
        db.fetch_pending_ready_nodes(Utc::now(), 100)
            .await
            .unwrap()
            .is_empty(),
        "terminal run should leave no pending ready nodes"
    );

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn claim_ready_nodes_for_announce_leases_once_per_window() {
    let path = std::env::temp_dir().join(format!(
        "runinator-announce-lease-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let workflow_id = db
        .upsert_workflow(&workflow("announce-lease-test"))
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
    // one already-due node and one future-dated node (an armed timeout).
    let due_event = runinator_models::orchestration::NewOrchestrationEvent::new(
        run.id,
        Some("due".into()),
        "local_target_poll",
        runinator_models::json!({}),
    );
    let due = db
        .enqueue_ready_node(due_event, "due".into(), now - Duration::seconds(1))
        .await
        .unwrap()
        .unwrap();
    let future_event = runinator_models::orchestration::NewOrchestrationEvent::new(
        run.id,
        Some("future".into()),
        "node_timeout_rearm",
        runinator_models::json!({}),
    );
    let future = db
        .enqueue_ready_node(future_event, "future".into(), now + Duration::seconds(600))
        .await
        .unwrap()
        .unwrap();

    // the first pass announces both rows; every pass inside the lease window announces nothing,
    // so a per-second publisher tick cannot flood a broker without in-flight dedupe.
    let first = db
        .claim_ready_nodes_for_announce(now, 30, 100)
        .await
        .unwrap();
    assert_eq!(first.len(), 2);
    let second = db
        .claim_ready_nodes_for_announce(now, 30, 100)
        .await
        .unwrap();
    assert!(second.is_empty(), "lease must suppress re-announcement");

    // once the due node's lease lapses it is re-announced (the lost-wake backstop); the future
    // node stays leased until ready_at + lease, so it is announced exactly once before it is due.
    let third = db
        .claim_ready_nodes_for_announce(now + Duration::seconds(31), 30, 100)
        .await
        .unwrap();
    assert_eq!(
        third.iter().map(|node| node.id).collect::<Vec<_>>(),
        vec![due.id],
        "only the due node's lease lapses inside its ready window"
    );
    let fourth = db
        .claim_ready_nodes_for_announce(now + Duration::seconds(631), 30, 100)
        .await
        .unwrap();
    assert!(
        fourth.iter().any(|node| node.id == future.id),
        "future node re-announces after ready_at + lease"
    );

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn enqueue_ready_node_collapses_due_generations_but_spares_future_rows() {
    let path = std::env::temp_dir().join(format!(
        "runinator-due-ready-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let workflow_id = db
        .upsert_workflow(&workflow("due-ready-test"))
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

    // arm a future-dated node timeout for the node first.
    let timeout_event = runinator_models::orchestration::NewOrchestrationEvent::new(
        run.id,
        Some("sync".into()),
        "node_timeout_rearm",
        runinator_models::json!({}),
    );
    db.enqueue_ready_node(
        timeout_event,
        "sync".into(),
        Utc::now() + Duration::seconds(600),
    )
    .await
    .unwrap()
    .unwrap();

    // re-arm the same node's due poll many times, as a lost-completion runaway would. each enqueue
    // collapses the prior due generation, so the pending set never grows past {future timeout, one
    // live due poll} — the invariant is enforced generically for every node kind at the db layer.
    for _ in 0..5 {
        let event = runinator_models::orchestration::NewOrchestrationEvent::new(
            run.id,
            Some("sync".into()),
            "local_target_poll",
            runinator_models::json!({}),
        );
        db.enqueue_ready_node(event, "sync".into(), Utc::now() - Duration::seconds(1))
            .await
            .unwrap()
            .unwrap();
    }

    let pending = db.fetch_pending_ready_nodes(Utc::now(), 100).await.unwrap();
    assert_eq!(
        pending.len(),
        2,
        "at most one live due poll plus the future timeout"
    );
    let now = Utc::now();
    assert_eq!(
        pending.iter().filter(|n| n.ready_at <= now).count(),
        1,
        "due poll generations collapse to a single live row"
    );
    assert_eq!(
        pending.iter().filter(|n| n.ready_at > now).count(),
        1,
        "the future-dated timeout row survives so timeout enforcement is preserved"
    );

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn settle_terminal_run_ready_nodes_spares_active_runs_and_respects_limit() {
    let path = std::env::temp_dir().join(format!(
        "runinator-terminal-reap-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let workflow_id = db
        .upsert_workflow(&workflow("terminal-reap-test"))
        .await
        .unwrap()
        .id
        .unwrap();
    let snapshot = db.fetch_workflow(workflow_id).await.unwrap().unwrap();
    let new_run = |snapshot: WorkflowDefinition| {
        db.create_workflow_run(
            workflow_id,
            snapshot,
            runinator_models::json!({}),
            runinator_models::json!({}),
            None,
            Default::default(),
        )
    };
    // a terminal run whose ready nodes were enqueued after it went terminal (so the inline cleanup
    // did not settle them — the crash-mid-transition orphan case the backstop exists for), plus an
    // active run with one legitimately pending node that must be spared.
    let terminal = new_run(snapshot.clone()).await.unwrap();
    db.update_workflow_run_status(terminal.id, WorkflowStatus::Failed, None, None, None)
        .await
        .unwrap();
    for node in ["a", "b", "c"] {
        let event = runinator_models::orchestration::NewOrchestrationEvent::new(
            terminal.id,
            Some(node.into()),
            "local_target_poll",
            runinator_models::json!({}),
        );
        db.enqueue_ready_node(event, node.into(), Utc::now())
            .await
            .unwrap()
            .unwrap();
    }
    let active = new_run(snapshot).await.unwrap();
    let active_event = runinator_models::orchestration::NewOrchestrationEvent::new(
        active.id,
        Some("start".into()),
        "workflow_run_created",
        runinator_models::json!({}),
    );
    db.enqueue_ready_node(active_event, "start".into(), Utc::now())
        .await
        .unwrap()
        .unwrap();

    // limit caps the batch: only one of the three terminal orphans settles this pass.
    let settled = db.settle_terminal_run_ready_nodes(1).await.unwrap();
    assert_eq!(settled, 1);
    // sweep the remaining two; then a further pass finds nothing left.
    let settled = db.settle_terminal_run_ready_nodes(1000).await.unwrap();
    assert_eq!(settled, 2);
    let settled = db.settle_terminal_run_ready_nodes(1000).await.unwrap();
    assert_eq!(settled, 0, "no orphans left for the terminal run");

    let pending = db.fetch_pending_ready_nodes(Utc::now(), 100).await.unwrap();
    assert_eq!(
        pending.len(),
        1,
        "only the active run's node remains pending"
    );
    assert_eq!(pending[0].workflow_run_id, active.id);

    let _ = fs::remove_file(path);
}
