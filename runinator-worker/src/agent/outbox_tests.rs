//! file outbox durability, bounds, and dead-letter behavior.

use chrono::Utc;
use runinator_comm::{EffectResult, EffectResultKind};
use runinator_models::workflow_vm::WorkflowEffectStatus;

use super::*;

fn effect_result_message() -> EffectResultMessage {
    let event_id = Uuid::now_v7();
    EffectResultMessage {
        result: EffectResult {
            workspace_commit: None,
            version: 1,
            event_id,
            effect_id: Uuid::now_v7(),
            workflow_run_id: Uuid::now_v7(),
            continuation_id: Uuid::now_v7(),
            attempt: 0,
            kind: EffectResultKind::Status {
                status: WorkflowEffectStatus::Succeeded,
                output: None,
                message: None,
            },
            timestamp: Utc::now(),
            trace_id: Uuid::new_v4(),
            notification_delivery_id: None,
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
    outbox.append_effect(effect_result_message()).unwrap();
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
    outbox.append_effect(effect_result_message()).unwrap();
    assert!(outbox.is_full());
    outbox.append_effect(effect_result_message()).unwrap();
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
    outbox.append_effect(effect_result_message()).unwrap();
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
    let first = effect_result_message();
    let second = effect_result_message();
    let first_id = first.result.event_id;
    let second_id = second.result.event_id;
    outbox.append_effect(first).unwrap();
    outbox.append_effect(second).unwrap();
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
            .receive_effect_result("server")
            .await
            .unwrap()
            .result
            .event_id,
        first_id
    );
    assert_eq!(
        broker
            .receive_effect_result("server")
            .await
            .unwrap()
            .result
            .event_id,
        second_id
    );
}

#[tokio::test]
async fn startup_drain_republishes_vm_effect_results() {
    use runinator_broker::{Broker, in_memory::InMemoryBroker};

    let directory = tempfile::tempdir().unwrap();
    let outbox = FileOutbox::open(directory.path().join("results.jsonl")).unwrap();
    let message = effect_result_message();
    let event_id = message.result.event_id;
    outbox.append_effect(message).unwrap();
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
            .receive_effect_result("server")
            .await
            .unwrap()
            .result
            .event_id,
        event_id
    );
}
