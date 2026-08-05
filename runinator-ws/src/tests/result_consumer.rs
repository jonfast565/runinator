//! the broker result consumer: acking duplicate deliveries while persisting once, dead-lettering a
//! poison event after its retries, and the backoff schedule between attempts.

use super::*;

#[tokio::test]
async fn result_consumer_acks_duplicate_deliveries_and_persists_results_once() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let node_run = create_node_run(&db).await;
    let command = action_command(node_run.workflow_run_id, node_run.id, &node_run.node_id);
    let chunk = WorkflowResultEvent::chunk(
        &command,
        NewRunChunk {
            stream: "log".into(),
            content: "hello".into(),
        },
    );
    let status = WorkflowResultEvent::status(
        &command,
        WorkflowStatus::Succeeded,
        Some(json!({ "ok": true })),
        Some("done".into()),
    );
    let artifact = WorkflowResultEvent::artifact(
        &command,
        NewRunArtifact {
            name: "report.json".into(),
            mime_type: "application/json".into(),
            size_bytes: 17,
            uri: "memory://report.json".into(),
            metadata: json!({ "source": "test" }),
        },
    );
    let broker = Arc::new(RecordingBroker::new());
    let broker_for_consumer: Arc<dyn Broker> = broker.clone();
    let publisher = runinator_engine::EnginePublisher::new(broker_for_consumer.clone());
    let shutdown = Arc::new(Notify::new());
    let consumer = tokio::spawn(crate::result_consumer::run_result_consumer(
        db.clone(),
        broker_for_consumer,
        publisher,
        shutdown.clone(),
    ));

    publish_duplicate_results(&broker, &[chunk.clone(), status.clone(), artifact.clone()]).await;
    wait_until(|| broker.result_acks().len() == 6).await;

    shutdown.notify_waiters();
    tokio::time::timeout(Duration::from_secs(1), consumer)
        .await
        .unwrap()
        .unwrap();

    let chunks = db
        .fetch_workflow_node_run_chunks(node_run.id, None, 100)
        .await
        .unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].stream, "log");
    assert_eq!(chunks[0].content, "hello");

    let node_run = db
        .fetch_workflow_node_run(node_run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(node_run.status, WorkflowStatus::Succeeded);
    assert_eq!(node_run.output_json, Some(json!({ "ok": true })));
    assert_eq!(node_run.message.as_deref(), Some("done"));

    let artifacts = db
        .fetch_workflow_node_run_artifacts(node_run.id)
        .await
        .unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].name, "report.json");
    assert_eq!(artifacts[0].uri, "memory://report.json");

    let received = broker.result_receives();
    let acked = broker.result_acks();
    assert_eq!(received.len(), 6);
    assert_eq!(acked.len(), 6);
    assert_eq!(received, acked);
    assert!(broker.result_nacks().is_empty());
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn result_consumer_dead_letters_poison_result_events_after_retries() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let node_run = create_node_run(&db).await;
    let command = action_command(
        node_run.workflow_run_id,
        node_run.id,
        "__force_result_persist_failure__",
    );
    let poison = WorkflowResultEvent::chunk(
        &command,
        NewRunChunk {
            stream: "log".into(),
            content: "poison".into(),
        },
    );
    let broker = Arc::new(RecordingBroker::new());
    let broker_for_consumer: Arc<dyn Broker> = broker.clone();
    let publisher = runinator_engine::EnginePublisher::new(broker_for_consumer.clone());
    let shutdown = Arc::new(Notify::new());
    let consumer = tokio::spawn(crate::result_consumer::run_result_consumer_with_policy(
        db.clone(),
        broker_for_consumer,
        publisher,
        shutdown.clone(),
        crate::result_consumer::ResultConsumerPolicy::new(2, Duration::from_millis(1)),
    ));

    broker
        .publish_result(ResultMessage {
            event: poison,
            dedupe_key: Some("poison-result".into()),
            enqueued_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    wait_until(|| broker.result_acks().len() == 1 && broker.result_nacks().len() == 1).await;

    shutdown.notify_waiters();
    tokio::time::timeout(Duration::from_secs(1), consumer)
        .await
        .unwrap()
        .unwrap();

    let chunks = db
        .fetch_workflow_node_run_chunks(node_run.id, None, 100)
        .await
        .unwrap();
    assert!(chunks.is_empty());

    // the poison event leaves a durable dead-letter record on the result channel.
    let dead_letters = db.fetch_dead_letters(None, 100).await.unwrap();
    assert_eq!(dead_letters.len(), 1);
    assert_eq!(
        dead_letters[0]
            .get("channel")
            .and_then(runinator_models::value::Value::as_str),
        Some("result")
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn result_consumer_backoff_grows_and_is_capped() {
    let policy = crate::result_consumer::ResultConsumerPolicy::new(5, Duration::from_millis(100));
    // full jitter keeps every delay within [0, base * 2^(attempt-1)] for early attempts.
    for attempt in 1..=4u32 {
        let ceiling = 100u128 * (1u128 << (attempt - 1));
        let backoff = policy.backoff_for(attempt).as_millis();
        assert!(
            backoff <= ceiling,
            "attempt {attempt} backoff {backoff} exceeded ceiling {ceiling}"
        );
    }
    // a large attempt count is clamped to the 30s max backoff and cannot overflow.
    let huge = policy.backoff_for(40).as_millis();
    assert!(huge <= 30_000, "backoff {huge} exceeded max_backoff");
}

#[tokio::test]
async fn output_node_promotes_artifacts_to_the_run() {
    let (db, path) = test_db().await;
    let node_run = create_node_run(&db).await;
    // a node artifact, then an output node promoting it to the run.
    let artifact = db
        .add_workflow_node_run_artifact(
            node_run.id,
            &NewRunArtifact {
                name: "dump.csv".into(),
                mime_type: "text/csv".into(),
                size_bytes: 42,
                uri: "memory://dump.csv".into(),
                metadata: json!({}),
            },
        )
        .await
        .unwrap();

    // the run-wide artifact fetch (used to build runtime context) sees it.
    let run_artifacts = db
        .fetch_workflow_node_run_artifacts_for_run(node_run.workflow_run_id)
        .await
        .unwrap();
    assert_eq!(run_artifacts.len(), 1);
    assert_eq!(run_artifacts[0].id, artifact.id);

    let stored = db
        .add_workflow_run_artifact(&NewWorkflowRunArtifact {
            workflow_run_id: node_run.workflow_run_id,
            node_id: node_run.node_id.clone(),
            artifact_id: artifact.id,
            name: "report".into(),
            mime_type: artifact.mime_type.clone(),
            size_bytes: artifact.size_bytes,
            uri: artifact.uri.clone(),
            metadata: json!({}),
        })
        .await
        .unwrap();
    assert_eq!(stored.name, "report");

    let artifacts = db
        .fetch_workflow_run_artifacts(node_run.workflow_run_id)
        .await
        .unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].artifact_id, artifact.id);
    assert_eq!(artifacts[0].name, "report");
    assert_eq!(artifacts[0].uri, "memory://dump.csv");
    let _ = std::fs::remove_file(path);
}
