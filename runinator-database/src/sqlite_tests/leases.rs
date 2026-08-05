//! mutual exclusion primitives: the executor lease (exclusive until stale or released) and the
//! idempotency claim (exclusive, replays a recorded result, frees on release or staleness).

use super::*;

#[tokio::test]
async fn executor_lease_is_mutually_exclusive_until_stale_or_released() {
    let path = std::env::temp_dir().join(format!(
        "runinator-executor-lease-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let workflow_id = db
        .upsert_workflow(&workflow("lease-test"))
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
        .create_workflow_node_run(run.id, "node-a".into(), runinator_models::json!({}), None)
        .await
        .unwrap();

    let register = |instance: &'static str| {
        let db = &db;
        async move {
            db.register_replica(
                runinator_models::replicas::ReplicaRegistrationRequest {
                    replica_type: runinator_models::replicas::ReplicaKind::Worker,
                    instance_id: instance.into(),
                    runtime_id: Uuid::new_v4().to_string(),
                    display_name: None,
                    host: None,
                    port: None,
                    base_path: None,
                    version: None,
                    attributes: runinator_models::json!({}),
                },
                None,
                &runinator_models::auth::AuthContext::disabled_admin(),
            )
            .await
            .unwrap()
            .replica_id
        }
    };
    let worker_a = register("worker-a").await;
    let worker_b = register("worker-b").await;
    let now = Utc::now();
    let stale_before = now - Duration::seconds(300);
    // both workers just registered, so they heartbeat well inside this window and stay live: these
    // cases exercise the deadline arm alone.
    let heartbeat_stale_before = now - Duration::seconds(30);

    // first claim wins.
    assert!(
        db.claim_workflow_node_run_executor(
            node_run.id,
            worker_a,
            now,
            stale_before,
            heartbeat_stale_before
        )
        .await
        .unwrap()
    );
    // a concurrent duplicate loses while the lease is fresh and its holder is live.
    assert!(
        !db.claim_workflow_node_run_executor(
            node_run.id,
            worker_b,
            now,
            stale_before,
            heartbeat_stale_before
        )
        .await
        .unwrap()
    );
    // once the prior claim ages past the cutoff, a retry may steal it.
    let future_cutoff = now + Duration::seconds(1);
    assert!(
        db.claim_workflow_node_run_executor(
            node_run.id,
            worker_b,
            now,
            future_cutoff,
            heartbeat_stale_before
        )
        .await
        .unwrap()
    );
    // a release by a replica that does not hold the lease is a no-op.
    db.release_workflow_node_run_executor(node_run.id, worker_a, Utc::now())
        .await
        .unwrap();
    assert!(
        !db.claim_workflow_node_run_executor(
            node_run.id,
            worker_a,
            Utc::now(),
            stale_before,
            heartbeat_stale_before
        )
        .await
        .unwrap()
    );
    // releasing by the holder frees the slot for the next attempt immediately.
    db.release_workflow_node_run_executor(node_run.id, worker_b, Utc::now())
        .await
        .unwrap();
    assert!(
        db.claim_workflow_node_run_executor(
            node_run.id,
            worker_a,
            Utc::now(),
            stale_before,
            heartbeat_stale_before
        )
        .await
        .unwrap()
    );

    let _ = fs::remove_file(path);
}

/// a crashed holder must not strand the node for its whole timeout window: the lease frees as soon
/// as the holder stops being live, well before the action deadline the claim also carries.
#[tokio::test]
async fn executor_lease_frees_when_the_holder_stops_heartbeating() {
    let path = std::env::temp_dir().join(format!(
        "runinator-executor-lease-heartbeat-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let workflow_id = db
        .upsert_workflow(&workflow("lease-heartbeat-test"))
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
        .create_workflow_node_run(run.id, "node-a".into(), runinator_models::json!({}), None)
        .await
        .unwrap();

    let register = |instance: &'static str| {
        let db = &db;
        async move {
            db.register_replica(
                runinator_models::replicas::ReplicaRegistrationRequest {
                    replica_type: runinator_models::replicas::ReplicaKind::Worker,
                    instance_id: instance.into(),
                    runtime_id: Uuid::new_v4().to_string(),
                    display_name: None,
                    host: None,
                    port: None,
                    base_path: None,
                    version: None,
                    attributes: runinator_models::json!({}),
                },
                None,
                &runinator_models::auth::AuthContext::disabled_admin(),
            )
            .await
            .unwrap()
            .replica_id
        }
    };
    let worker_a = register("worker-a").await;
    let worker_b = register("worker-b").await;
    let now = Utc::now();
    // a long-running action: its deadline is far in the future, so the deadline arm cannot free the
    // lease. only the holder's liveness can.
    let action_deadline = now - Duration::seconds(3600);

    assert!(
        db.claim_workflow_node_run_executor(
            node_run.id,
            worker_a,
            now,
            action_deadline,
            now - Duration::seconds(30)
        )
        .await
        .unwrap()
    );
    // while the holder keeps heartbeating, the lease holds regardless of how long the action runs.
    assert!(
        !db.claim_workflow_node_run_executor(
            node_run.id,
            worker_b,
            now,
            action_deadline,
            now - Duration::seconds(30)
        )
        .await
        .unwrap()
    );

    // the holder crashes: its last heartbeat now predates the liveness cutoff, and the lease frees
    // without waiting out the action deadline.
    assert!(
        db.claim_workflow_node_run_executor(
            node_run.id,
            worker_b,
            now,
            action_deadline,
            now + Duration::seconds(1)
        )
        .await
        .unwrap()
    );

    // a graceful shutdown is the same story by a different signal: the holder is marked offline, so
    // its lease frees on the next claim even though its heartbeat is still fresh.
    let worker_b_record = db.fetch_replica(worker_b).await.unwrap().unwrap();
    db.mark_replica_offline(worker_b, worker_b_record.runtime_id)
        .await
        .unwrap();
    assert!(
        db.claim_workflow_node_run_executor(
            node_run.id,
            worker_a,
            now,
            action_deadline,
            now - Duration::seconds(30)
        )
        .await
        .unwrap()
    );

    let _ = fs::remove_file(path);
}

/// the reservation half of `.idempotent(key: ...)`: exactly one claimant acquires, the loser is told
/// who holds it, and a recorded result turns every later claim into a replay.
#[tokio::test]
async fn idempotency_claim_is_exclusive_and_replays_a_recorded_result() {
    let path = std::env::temp_dir().join(format!(
        "runinator-idempotency-claim-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let scope = runinator_models::orchestration::ACTION_IDEMPOTENCY_SCOPE.to_string();
    let key = "workflow:abc:invoice-42".to_string();
    let first = Uuid::now_v7();
    let second = Uuid::now_v7();
    let now = Utc::now();
    let stale_before = now - Duration::seconds(300);

    // first claimant acquires.
    assert_eq!(
        db.claim_idempotency_key(scope.clone(), key.clone(), first, now, stale_before)
            .await
            .unwrap(),
        IdempotencyClaim::Acquired
    );
    // a different node run loses while the reservation is live, and learns who holds it.
    assert_eq!(
        db.claim_idempotency_key(scope.clone(), key.clone(), second, now, stale_before)
            .await
            .unwrap(),
        IdempotencyClaim::Held {
            owner_node_run_id: first
        }
    );
    // the owner re-claiming its own reservation still acquires: a redelivery of the same node run may
    // have died before doing any work, and refusing it would strand the node.
    assert_eq!(
        db.claim_idempotency_key(scope.clone(), key.clone(), first, now, stale_before)
            .await
            .unwrap(),
        IdempotencyClaim::Acquired
    );

    // recording the result turns every later claim into a replay — including the owner's own
    // redelivery, which is what stops a failed status publish from re-running the side effect.
    let result = runinator_models::json!({ "success": true, "message": "charged" });
    assert!(
        db.complete_idempotency_key(scope.clone(), key.clone(), first, result.clone(), now)
            .await
            .unwrap()
    );
    for claimant in [first, second] {
        assert_eq!(
            db.claim_idempotency_key(scope.clone(), key.clone(), claimant, now, stale_before)
                .await
                .unwrap(),
            IdempotencyClaim::Completed {
                result: result.clone()
            }
        );
    }
    // a second completion cannot overwrite the first.
    assert!(
        !db.complete_idempotency_key(
            scope.clone(),
            key.clone(),
            first,
            runinator_models::json!({ "success": false }),
            now
        )
        .await
        .unwrap()
    );

    let _ = fs::remove_file(path);
}

/// an unfinished reservation must not outlive its usefulness: releasing frees it immediately after a
/// failed attempt, and a worker that died holding one has it taken over once it ages out.
#[tokio::test]
async fn idempotency_reservation_frees_on_release_and_on_staleness() {
    let path = std::env::temp_dir().join(format!(
        "runinator-idempotency-release-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let scope = runinator_models::orchestration::ACTION_IDEMPOTENCY_SCOPE.to_string();
    let key = "workflow:abc:invoice-43".to_string();
    let first = Uuid::now_v7();
    let second = Uuid::now_v7();
    let now = Utc::now();
    let fresh = now - Duration::seconds(300);

    // a failed attempt releases, so the next claimant is not held off at all.
    db.claim_idempotency_key(scope.clone(), key.clone(), first, now, fresh)
        .await
        .unwrap();
    assert!(
        db.release_idempotency_key(scope.clone(), key.clone(), first)
            .await
            .unwrap()
    );
    assert_eq!(
        db.claim_idempotency_key(scope.clone(), key.clone(), second, now, fresh)
            .await
            .unwrap(),
        IdempotencyClaim::Acquired
    );
    // a release by someone who does not hold it cannot free the live reservation.
    assert!(
        !db.release_idempotency_key(scope.clone(), key.clone(), first)
            .await
            .unwrap()
    );

    // a holder that died leaves its reservation behind; once it predates the cutoff it is takeable.
    let third = Uuid::now_v7();
    assert_eq!(
        db.claim_idempotency_key(scope.clone(), key.clone(), third, now, fresh)
            .await
            .unwrap(),
        IdempotencyClaim::Held {
            owner_node_run_id: second
        }
    );
    assert_eq!(
        db.claim_idempotency_key(
            scope.clone(),
            key.clone(),
            third,
            now,
            now + Duration::seconds(1)
        )
        .await
        .unwrap(),
        IdempotencyClaim::Acquired
    );
    // a completed result is never taken over, however stale the row looks.
    db.complete_idempotency_key(
        scope.clone(),
        key.clone(),
        third,
        runinator_models::json!({ "success": true }),
        now,
    )
    .await
    .unwrap();
    assert!(matches!(
        db.claim_idempotency_key(
            scope.clone(),
            key.clone(),
            first,
            now,
            now + Duration::seconds(1)
        )
        .await
        .unwrap(),
        IdempotencyClaim::Completed { .. }
    ));

    let _ = fs::remove_file(path);
}
