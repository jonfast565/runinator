//! result kinds admitted to the durable outage buffer.

use runinator_models::runs::{NewRunArtifact, NewRunChunk};
use uuid::Uuid;

use super::*;

fn event(kind: WorkflowResultEventKind) -> WorkflowResultEvent {
    WorkflowResultEvent {
        event_id: Uuid::now_v7(),
        command_id: Uuid::new_v4(),
        workflow_run_id: Uuid::new_v4(),
        workflow_node_run_id: Uuid::new_v4(),
        node_id: "task".to_string(),
        attempt: 1,
        kind,
        timestamp: Utc::now(),
        trace_id: Uuid::new_v4(),
        notification_delivery_id: None,
        invocation_call_id: None,
        task_run_id: None,
    }
}

#[test]
fn only_terminal_statuses_and_artifacts_are_buffered() {
    assert!(should_buffer(&event(WorkflowResultEventKind::Status {
        status: WorkflowStatus::Succeeded,
        output_json: None,
        message: None,
    })));
    assert!(!should_buffer(&event(WorkflowResultEventKind::Status {
        status: WorkflowStatus::Running,
        output_json: None,
        message: None,
    })));
    assert!(should_buffer(&event(WorkflowResultEventKind::Artifact {
        artifact: NewRunArtifact {
            name: "report".to_string(),
            mime_type: "text/plain".to_string(),
            size_bytes: 4,
            uri: "file:///report".to_string(),
            metadata: Value::default(),
        },
    })));
    assert!(!should_buffer(&event(WorkflowResultEventKind::Chunk {
        chunk: NewRunChunk {
            stream: "log".to_string(),
            content: "hello".to_string(),
        },
    })));
}
