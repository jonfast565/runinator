use crate::{
    ActionCommand, ActionTarget, ControlCommand, ControlKind, WakeCommand, WireCodec,
    WorkflowResultEvent, WorkflowResultEventKind, WsIngressCommand,
};
use chrono::Utc;
use runinator_models::{json, runs::NewRunChunk, workflows::WorkflowAction};
use uuid::Uuid;

#[test]
fn wake_command_round_trips_with_json_and_dedupes_by_node() {
    let source = Uuid::new_v4();
    let ready_node_id = Uuid::now_v7();
    let workflow_run_id = Uuid::now_v7();
    let command = WakeCommand::new(
        ready_node_id,
        workflow_run_id,
        "node-a".into(),
        Utc::now(),
        source,
        Uuid::now_v7(),
    );
    let encoded = command.to_wire().unwrap();
    let decoded = WakeCommand::from_wire(&encoded).unwrap();

    assert_eq!(decoded.ready_node_id, ready_node_id);
    assert_eq!(decoded.workflow_run_id, workflow_run_id);
    assert_eq!(decoded.dedupe_key(), format!("{ready_node_id}:{source}"));
}

#[test]
fn ws_ingress_command_round_trips_and_dedupes_per_kind() {
    let ready_node_id = Uuid::now_v7();
    let workflow_run_id = Uuid::now_v7();
    let drive = WsIngressCommand::drive(
        ready_node_id,
        workflow_run_id,
        "node-a".into(),
        Uuid::now_v7(),
    );
    let decoded = WsIngressCommand::from_wire(&drive.to_wire().unwrap()).unwrap();
    assert!(matches!(
        decoded,
        WsIngressCommand::Drive { ready_node_id: rid, .. } if rid == ready_node_id
    ));
    assert_eq!(drive.dedupe_key(), format!("drive:{ready_node_id}"));

    let control = WsIngressCommand::control(workflow_run_id, ControlKind::Cancel);
    assert_eq!(
        control.dedupe_key(),
        format!("control:{workflow_run_id}:Cancel")
    );
}

#[test]
fn control_command_round_trips_its_target_and_defaults_older_messages_to_any() {
    let workflow_run_id = Uuid::now_v7();
    let replica_id = Uuid::now_v7();
    let command =
        ControlCommand::for_node_run(workflow_run_id, Uuid::now_v7(), ControlKind::Cancel)
            .targeting_replica(replica_id);
    let decoded = ControlCommand::from_wire(&command.to_wire().unwrap()).unwrap();
    assert_eq!(decoded.target, ActionTarget::Replica { replica_id });

    // a pre-targeting message (no `target` field) must deserialize as `Any`.
    let legacy = format!(r#"{{"workflow_run_id":"{workflow_run_id}","kind":"cancel"}}"#);
    let decoded: ControlCommand = serde_json::from_str(&legacy).unwrap();
    assert_eq!(decoded.target, ActionTarget::Any);
}

#[test]
fn workflow_result_events_round_trip_with_json() {
    let workflow_node_run_id = Uuid::now_v7();
    let command = ActionCommand {
        command_id: Uuid::new_v4(),
        workflow_run_id: Uuid::now_v7(),
        workflow_node_run_id,
        node_id: "node-a".into(),
        action: WorkflowAction {
            provider: "test".into(),
            function: "execute".into(),
            timeout_seconds: 60,
            configuration: runinator_models::workflows::WorkflowObject::default(),
            mcp_enabled: false,
            tags: Vec::new(),
            required_labels: Default::default(),
        },
        attempt: 1,
        parameters: json!({}),
        target: Default::default(),
        trace_id: Uuid::nil(),
        trace_context: Default::default(),
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
    assert_eq!(decoded.workflow_node_run_id, workflow_node_run_id);
    assert_eq!(decoded.attempt, 1);
    match decoded.kind {
        WorkflowResultEventKind::Chunk { chunk } => {
            assert_eq!(chunk.stream, "log");
            assert_eq!(chunk.content, "hello");
        }
        _ => panic!("expected chunk result event"),
    }

    // an older message with no attempt field decodes to 0 (unknown), never an error.
    let mut legacy = serde_json::to_value(&event).unwrap();
    legacy.as_object_mut().unwrap().remove("attempt");
    let decoded: WorkflowResultEvent = serde_json::from_value(legacy).unwrap();
    assert_eq!(decoded.attempt, 0);
}
