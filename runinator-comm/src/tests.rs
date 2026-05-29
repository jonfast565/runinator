use prost::Message;

use crate::{
    ActionCommand, ControlKind, WireCodec, WorkflowResultEvent, WorkflowResultEventKind,
    worker_control::{
        SchedulerControlAck, WorkerControlActionKind, WorkerControlEvent, WorkerControlEventKind,
    },
};
use runinator_models::{runs::NewRunChunk, workflows::WorkflowAction};
use serde_json::json;
use uuid::Uuid;

#[test]
fn worker_control_events_round_trip_with_protobuf() {
    let event = WorkerControlEvent::new("worker-1", WorkerControlEventKind::ControlRequested, 42)
        .with_workflow_run_id(10)
        .with_workflow_node_run_id(20)
        .with_node_id("node-a")
        .with_control_kind(ControlKind::Cancel)
        .with_message("cancel requested");

    let mut encoded = Vec::new();
    event.encode(&mut encoded).unwrap();
    let decoded = WorkerControlEvent::decode(encoded.as_slice()).unwrap();

    assert_eq!(decoded.worker_id, "worker-1");
    assert_eq!(
        WorkerControlEventKind::try_from(decoded.kind).unwrap(),
        WorkerControlEventKind::ControlRequested
    );
    assert_eq!(
        WorkerControlActionKind::try_from(decoded.control_kind.unwrap()).unwrap(),
        WorkerControlActionKind::Cancel
    );
    assert_eq!(decoded.workflow_run_id, Some(10));
}

#[test]
fn scheduler_control_ack_round_trips_with_protobuf() {
    let ack = SchedulerControlAck::rejected("invalid control event");
    let mut encoded = Vec::new();
    ack.encode(&mut encoded).unwrap();
    let decoded = SchedulerControlAck::decode(encoded.as_slice()).unwrap();

    assert!(!decoded.accepted);
    assert_eq!(decoded.message, "invalid control event");
}

#[test]
fn workflow_result_events_round_trip_with_json() {
    let command = ActionCommand {
        command_id: Uuid::new_v4(),
        workflow_run_id: 10,
        workflow_node_run_id: 20,
        node_id: "node-a".into(),
        action: WorkflowAction {
            provider: "test".into(),
            function: "execute".into(),
            timeout_seconds: 60,
            configuration: json!({}),
            mcp_enabled: false,
            tags: Vec::new(),
        },
        attempt: 1,
        parameters: json!({}),
    };
    let event = WorkflowResultEvent::chunk(
        &command,
        NewRunChunk {
            stream: "log".into(),
            content: "hello".into(),
        },
    );

    let encoded = event.to_wire().unwrap();
    let decoded = WorkflowResultEvent::from_wire(&encoded).unwrap();

    assert_eq!(decoded.command_id, command.command_id);
    assert_eq!(decoded.workflow_node_run_id, 20);
    match decoded.kind {
        WorkflowResultEventKind::Chunk { chunk } => {
            assert_eq!(chunk.stream, "log");
            assert_eq!(chunk.content, "hello");
        }
        _ => panic!("expected chunk result event"),
    }
}
