use crate::{providers::ProviderMetadata, workflows::*};
use serde_json::json;

#[test]
fn workflow_status_terminal_and_active() {
    assert!(WorkflowStatus::Succeeded.is_terminal());
    assert!(!WorkflowStatus::Succeeded.is_active());
    assert!(WorkflowStatus::Failed.is_terminal());
    assert!(WorkflowStatus::TimedOut.is_terminal());
    assert!(WorkflowStatus::Canceled.is_terminal());

    assert!(!WorkflowStatus::Queued.is_terminal());
    assert!(WorkflowStatus::Queued.is_active());
    assert!(WorkflowStatus::Running.is_active());
    assert!(WorkflowStatus::Waiting.is_active());
    assert!(WorkflowStatus::ApprovalRequired.is_active());

    assert!(!WorkflowStatus::Blocked.is_terminal());
    assert!(!WorkflowStatus::Blocked.is_active());
}

#[test]
fn workflow_status_serialization() {
    let status = WorkflowStatus::ApprovalRequired;
    let serialized = serde_json::to_string(&status).unwrap();
    assert_eq!(serialized, "\"approval_required\"");

    let deserialized: WorkflowStatus = serde_json::from_str("\"approval_required\"").unwrap();
    assert_eq!(deserialized, WorkflowStatus::ApprovalRequired);
}

#[test]
fn workflow_node_serialization() {
    let node_json = json!({
        "id": "test-node",
        "kind": "task",
        "task_id": 123,
        "transitions": {
            "on_success": { "$node": "next-node" }
        }
    });
    let node: WorkflowNode = serde_json::from_value(node_json).unwrap();
    assert_eq!(node.id, "test-node");
    assert_eq!(node.kind, WorkflowNodeKind::Task);
    assert_eq!(node.task_id, Some(123));
    assert_eq!(
        node.transitions
            .on_success
            .as_ref()
            .map(WorkflowNodeRef::as_str),
        Some("next-node")
    );
}

#[test]
fn workflow_node_kind_accepts_rich_control_flow_nodes() {
    for (kind, expected) in [
        ("switch", WorkflowNodeKind::Switch),
        ("parallel", WorkflowNodeKind::Parallel),
        ("join", WorkflowNodeKind::Join),
        ("try", WorkflowNodeKind::Try),
        ("map", WorkflowNodeKind::Map),
        ("race", WorkflowNodeKind::Race),
        ("emit", WorkflowNodeKind::Emit),
    ] {
        let node: WorkflowNode = serde_json::from_value(json!({
            "id": kind,
            "kind": kind,
            "parameters": {}
        }))
        .unwrap();
        assert_eq!(node.kind, expected);
    }
}

#[test]
fn provider_metadata_accepts_catalog_provider_name() {
    let metadata: ProviderMetadata = serde_json::from_value(json!({
        "provider_name": "git",
        "actions": [
            { "function_name": "diff", "description": "Get diff" }
        ]
    }))
    .unwrap();

    assert_eq!(metadata.name, "git");
    assert_eq!(metadata.actions[0].function_name, "diff");
    assert!(metadata.metadata.credential_scopes.is_empty());
}
