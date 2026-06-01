use crate::{
    ActionCommand, ControlKind, WakeCommand, WireCodec, WorkflowResultEvent,
    WorkflowResultEventKind, WsIngressCommand,
};
use chrono::Utc;
use runinator_models::{json, runs::NewRunChunk, workflows::WorkflowAction};
use uuid::Uuid;

#[test]
fn wake_command_round_trips_with_json_and_dedupes_by_node() {
    let source = Uuid::new_v4();
    let command = WakeCommand::new(7, 42, "node-a".into(), Utc::now(), source);
    let encoded = command.to_wire().unwrap();
    let decoded = WakeCommand::from_wire(&encoded).unwrap();

    assert_eq!(decoded.ready_node_id, 7);
    assert_eq!(decoded.workflow_run_id, 42);
    assert_eq!(decoded.dedupe_key(), format!("7:{source}"));
}

#[test]
fn ws_ingress_command_round_trips_and_dedupes_per_kind() {
    let drive = WsIngressCommand::drive(7, 42, "node-a".into());
    let decoded = WsIngressCommand::from_wire(&drive.to_wire().unwrap()).unwrap();
    assert!(matches!(
        decoded,
        WsIngressCommand::Drive {
            ready_node_id: 7,
            ..
        }
    ));
    assert_eq!(drive.dedupe_key(), "drive:7");

    let control = WsIngressCommand::control(42, ControlKind::Cancel);
    assert_eq!(control.dedupe_key(), "control:42:Cancel");
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
            configuration: runinator_models::workflows::WorkflowObject::default(),
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
