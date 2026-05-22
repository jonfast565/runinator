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
    assert!(WorkflowStatus::DebugPaused.is_active());
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

    let debug: WorkflowStatus = serde_json::from_str("\"debug_paused\"").unwrap();
    assert_eq!(debug, WorkflowStatus::DebugPaused);
    assert_eq!(debug.as_str(), "debug_paused");
}

#[test]
fn workflow_node_serialization() {
    let node_json = json!({
        "id": "test-node",
        "kind": "action",
        "action": {
            "provider": "console",
            "function": "run",
            "timeout_seconds": 60,
            "configuration": {}
        },
        "transitions": {
            "on_success": { "$node": "next-node" }
        }
    });
    let node: WorkflowNode = serde_json::from_value(node_json).unwrap();
    assert_eq!(node.id, "test-node");
    assert_eq!(node.kind, WorkflowNodeKind::Action);
    assert_eq!(
        node.action.as_ref().map(|action| action.provider.as_str()),
        Some("console")
    );
    assert_eq!(
        node.transitions
            .on_success
            .as_ref()
            .map(WorkflowNodeRef::as_str),
        Some("next-node")
    );
}

#[test]
fn workflow_node_accepts_reentry_configuration() {
    let node: WorkflowNode = serde_json::from_value(json!({
        "id": "build",
        "kind": "action",
        "action": {
            "provider": "console",
            "function": "run",
            "timeout_seconds": 60,
            "configuration": {}
        },
        "reentry": {
            "enabled": true,
            "max_visits": 3,
            "on_exhausted": { "$node": "deferred" }
        }
    }))
    .unwrap();

    assert!(node.reentry.enabled);
    assert_eq!(node.reentry.max_visits, 3);
    assert_eq!(
        node.reentry
            .on_exhausted
            .as_ref()
            .map(WorkflowNodeRef::as_str),
        Some("deferred")
    );
}

#[test]
fn workflow_action_accepts_inline_configuration_items() {
    let node: WorkflowNode = serde_json::from_value(json!({
        "id": "build",
        "kind": "action",
        "action": {
            "provider": "console",
            "function": "run",
            "timeout_seconds": 60,
            "configuration": {
                "shell": "bash"
            },
            "command": "echo hello"
        }
    }))
    .unwrap();

    let action = node.action.unwrap();
    assert_eq!(action.configuration["shell"], "bash");
    assert_eq!(action.configuration["command"], "echo hello");
}

#[test]
fn workflow_action_rejects_task_metadata_shape() {
    let err = serde_json::from_value::<WorkflowNode>(json!({
        "id": "build",
        "kind": "action",
        "action": {
            "provider": "console",
            "function": "run",
            "metadata": {}
        }
    }))
    .unwrap_err();

    assert!(
        err.to_string()
            .contains("action metadata is no longer supported")
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
        ("config", WorkflowNodeKind::Config),
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

#[test]
fn workflow_bundle_uses_importer_shape() {
    let bundle: WorkflowBundle = serde_json::from_value(json!({
        "workflows": [
            {
                "id": 7,
                "name": "dev workflow",
                "version": 1,
                "enabled": true,
                "input_schema": {},
                "definition": {}
            }
        ],
        "triggers": [
            {
                "id": 3,
                "workflow_id": 7,
                "kind": "manual",
                "enabled": true,
                "configuration": {},
                "next_execution": null,
                "blackout_start": null,
                "blackout_end": null,
                "metadata": {}
            }
        ]
    }))
    .unwrap();

    assert_eq!(bundle.workflows[0].id, Some(7));
    assert_eq!(bundle.triggers[0].workflow_id, 7);

    let value = serde_json::to_value(bundle).unwrap();
    assert!(value.get("workflows").is_some());
    assert!(value.get("triggers").is_some());
}
