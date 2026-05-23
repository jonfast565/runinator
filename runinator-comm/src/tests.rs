use prost::Message;

use crate::{
    ControlKind,
    worker_control::{
        SchedulerControlAck, WorkerControlActionKind, WorkerControlEvent, WorkerControlEventKind,
    },
};

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
