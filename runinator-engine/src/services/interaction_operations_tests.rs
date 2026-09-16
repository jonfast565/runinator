//! action discovery for each durable human-control effect.

use super::*;
use runinator_models::{
    orchestration::GateKind,
    value::Value,
    workflow_vm::{
        WORKFLOW_EFFECT_PROTOCOL_VERSION, WorkflowEffect, WorkflowEffectRequest,
        WorkflowEffectStatus,
    },
    workflows::WorkflowRetry,
};
use uuid::Uuid;

fn effect(request: WorkflowEffectRequest) -> WorkflowEffect {
    WorkflowEffect {
        version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
        id: Uuid::now_v7(),
        workflow_run_id: Uuid::now_v7(),
        continuation_id: Uuid::now_v7(),
        sequence: 0,
        attempt: 3,
        node_id: Some("control".into()),
        timeline_category: Default::default(),
        request,
        status: WorkflowEffectStatus::Running,
        current_executor_replica_id: None,
        last_executor_replica_id: None,
        result: None,
        message: None,
        created_at: 0,
        updated_at: 0,
        finished_at: None,
    }
}

fn action_ids(request: WorkflowEffectRequest) -> Vec<String> {
    interaction_operations::interaction_contract(&effect(request))
        .expect("request is interactive")
        .1
        .into_iter()
        .map(|action| action.id)
        .collect()
}

#[test]
fn durable_human_controls_publish_their_native_actions() {
    assert_eq!(
        action_ids(WorkflowEffectRequest::Approval {
            prompt: Value::String("deploy?".into()),
            expires_at: None,
        }),
        ["approve", "reject"]
    );
    assert_eq!(
        action_ids(WorkflowEffectRequest::Gate {
            kind: GateKind::Manual,
            condition: Default::default(),
            poll_interval_seconds: 30,
            deadline_seconds: None,
            continue_on_timeout: false,
            label: None,
            metadata: Value::Null,
        }),
        ["open", "close"]
    );
    assert_eq!(
        action_ids(WorkflowEffectRequest::Input {
            prompt: Some("parameters".into()),
            schema: Value::Null,
        }),
        ["submit"]
    );
    assert_eq!(
        action_ids(WorkflowEffectRequest::Signal {
            key: "release".into(),
            filter: None,
        }),
        ["signal"]
    );
}

#[test]
fn terminal_steering_requires_the_effect_to_opt_in() {
    let terminal = |interactive| {
        effect(WorkflowEffectRequest::Action {
            provider: "console".into(),
            function: "run".into(),
            input: runinator_models::json!({ "interactive": interactive }),
            timeout_seconds: None,
            retry: WorkflowRetry::default(),
            tags: Vec::new(),
            required_labels: Default::default(),
            workspace_affinity: None,
            execution_profile: None,
            idempotency_key: None,
            function_binding: None,
        })
    };
    assert!(interaction_operations::interaction_contract(&terminal(false)).is_none());
    assert_eq!(
        interaction_operations::interaction_contract(&terminal(true))
            .expect("interactive terminal")
            .1[0]
            .id,
        "steer"
    );
}
