//! file outbox durability, bounds, and dead-letter behavior.

use chrono::Utc;
use runinator_comm::{WorkflowResultEvent, WorkflowResultEventKind};
use runinator_models::workflows::WorkflowStatus;

use super::*;

fn result_message() -> ResultMessage {
    let event_id = Uuid::now_v7();
    ResultMessage {
        event: WorkflowResultEvent {
            event_id,
            command_id: Uuid::new_v4(),
            workflow_run_id: Uuid::new_v4(),
            workflow_node_run_id: Uuid::new_v4(),
            node_id: "task".to_string(),
            attempt: 1,
            kind: WorkflowResultEventKind::Status {
                status: WorkflowStatus::Succeeded,
                output_json: None,
                message: None,
            },
            timestamp: Utc::now(),
            trace_id: Uuid::new_v4(),
            notification_delivery_id: None,
            invocation_call_id: None,
        },
        dedupe_key: Some(event_id.to_string()),
        enqueued_at: Utc::now(),
    }
}

#[test]
fn append_survives_reopen_and_ack_rewrites_the_queue() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("results.jsonl");
    let outbox = FileOutbox::open(&path).unwrap();
    outbox.append(result_message()).unwrap();
    assert_eq!(outbox.depth(), 1);
    drop(outbox);

    let reopened = FileOutbox::open(&path).unwrap();
    let entry = reopened.next().unwrap().unwrap();
    reopened.acknowledge(entry.id).unwrap();
    assert_eq!(reopened.depth(), 0);
    assert!(fs::read(&path).unwrap().is_empty());
}

#[test]
fn configured_count_is_a_hard_pending_queue_cap() {
    let directory = tempfile::tempdir().unwrap();
    let outbox = FileOutbox::with_limits(
        directory.path().join("results.jsonl"),
        1,
        DEFAULT_MAX_BYTES,
        DEFAULT_MAX_ATTEMPTS,
    )
    .unwrap();
    outbox.append(result_message()).unwrap();
    assert!(outbox.is_full());
    outbox.append(result_message()).unwrap();
    assert_eq!(outbox.depth(), 1);
    let dead_letters =
        fs::read_to_string(directory.path().join("results.dead-letter.jsonl")).unwrap();
    assert!(dead_letters.contains("result outbox capacity exceeded"));
}

#[test]
fn exhausted_delivery_moves_to_a_fsynced_dead_letter() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("results.jsonl");
    let outbox = FileOutbox::with_limits(&path, 10, DEFAULT_MAX_BYTES, 1).unwrap();
    outbox.append(result_message()).unwrap();
    let id = outbox.next().unwrap().unwrap().id;
    outbox
        .record_failure(id, "broker down".to_string())
        .unwrap();
    assert_eq!(outbox.depth(), 0);
    let dead_letters = fs::read_to_string(path.with_extension("dead-letter.jsonl")).unwrap();
    assert!(dead_letters.contains("broker down"));
}

#[tokio::test]
async fn startup_drain_preserves_recorded_order() {
    use runinator_broker::{Broker, in_memory::InMemoryBroker};

    let directory = tempfile::tempdir().unwrap();
    let outbox = FileOutbox::open(directory.path().join("results.jsonl")).unwrap();
    let first = result_message();
    let second = result_message();
    let first_id = first.event.event_id;
    let second_id = second.event.event_id;
    outbox.append(first).unwrap();
    outbox.append(second).unwrap();
    let broker = InMemoryBroker::new();
    let shutdown = tokio::sync::Notify::new();

    assert!(
        drain_before_work(&outbox, &broker, &shutdown)
            .await
            .unwrap()
    );
    assert_eq!(outbox.depth(), 0);
    assert_eq!(
        broker
            .receive_result("server")
            .await
            .unwrap()
            .event
            .event_id,
        first_id
    );
    assert_eq!(
        broker
            .receive_result("server")
            .await
            .unwrap()
            .event
            .event_id,
        second_id
    );
}
