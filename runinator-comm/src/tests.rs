use crate::{
    ActionTarget, AgentDirectiveKind, ControlCommand, ControlKind, EffectCommand, EffectExecutor,
    EffectResult, EffectResultKind, GossipMessage, UiEvent, UiEventKind, WakeCommand,
    WebServiceAnnouncement, WireCodec, WsIngressCommand,
};
use chrono::Utc;
use runinator_models::json;
use uuid::Uuid;

#[test]
fn wake_command_round_trips_with_json_and_carries_its_effect_result() {
    let effect_id = Uuid::now_v7();
    let workflow_run_id = Uuid::now_v7();
    let due_at = Utc::now();
    let command = WakeCommand::new(
        due_at,
        EffectResult {
            workspace_commit: None,
            version: crate::WORKFLOW_EFFECT_PROTOCOL_VERSION,
            event_id: Uuid::now_v7(),
            effect_id,
            workflow_run_id,
            continuation_id: Uuid::now_v7(),
            attempt: 0,
            kind: EffectResultKind::Status {
                status: runinator_models::workflow_vm::WorkflowEffectStatus::Succeeded,
                output: None,
                message: None,
            },
            timestamp: due_at,
            trace_id: Uuid::now_v7(),
            notification_delivery_id: None,
        },
        Uuid::now_v7(),
    );
    let encoded = command.to_wire().unwrap();
    let decoded = WakeCommand::from_wire(&encoded).unwrap();

    assert_eq!(decoded.effect_id(), effect_id);
    assert_eq!(decoded.workflow_run_id(), workflow_run_id);
    assert_eq!(decoded.dedupe_key(), format!("{effect_id}:0"));
}

#[test]
fn unknown_agent_directive_types_deserialize_as_unsupported_candidates() {
    let kind: AgentDirectiveKind =
        serde_json::from_str(r#"{ "type": "future_operation", "new_field": true }"#).unwrap();
    assert!(matches!(kind, AgentDirectiveKind::Unknown));
}

#[test]
fn ws_ingress_command_round_trips_and_dedupes_per_kind() {
    let workflow_run_id = Uuid::now_v7();
    let effect_id = Uuid::now_v7();
    let result = EffectResult {
        workspace_commit: None,
        version: crate::WORKFLOW_EFFECT_PROTOCOL_VERSION,
        event_id: Uuid::now_v7(),
        effect_id,
        workflow_run_id,
        continuation_id: Uuid::now_v7(),
        attempt: 2,
        kind: EffectResultKind::Status {
            status: runinator_models::workflow_vm::WorkflowEffectStatus::Succeeded,
            output: None,
            message: None,
        },
        timestamp: chrono::Utc::now(),
        trace_id: Uuid::now_v7(),
        notification_delivery_id: None,
    };
    let settle = WsIngressCommand::settle_effect(result.clone(), Uuid::now_v7());
    let decoded = WsIngressCommand::from_wire(&settle.to_wire().unwrap()).unwrap();
    assert!(matches!(
        decoded,
        WsIngressCommand::SettleEffect { result, .. } if result.effect_id == effect_id
    ));
    assert_eq!(settle.dedupe_key(), format!("settle:{effect_id}:2"));

    let control = WsIngressCommand::control(workflow_run_id, ControlKind::Cancel);
    assert_eq!(
        control.dedupe_key(),
        format!("control:{workflow_run_id}:Cancel")
    );
}

#[test]
fn wake_command_dedupes_per_effect_attempt_and_carries_its_result() {
    let effect_id = Uuid::now_v7();
    let due_at = chrono::Utc::now() + chrono::Duration::seconds(30);
    let result = EffectResult {
        workspace_commit: None,
        version: crate::WORKFLOW_EFFECT_PROTOCOL_VERSION,
        event_id: Uuid::now_v7(),
        effect_id,
        workflow_run_id: Uuid::now_v7(),
        continuation_id: Uuid::now_v7(),
        attempt: 0,
        kind: EffectResultKind::Status {
            status: runinator_models::workflow_vm::WorkflowEffectStatus::Succeeded,
            output: None,
            message: None,
        },
        timestamp: due_at,
        trace_id: Uuid::now_v7(),
        notification_delivery_id: None,
    };
    let wake = WakeCommand::new(due_at, result, Uuid::now_v7());

    assert_eq!(wake.effect_id(), effect_id);
    assert_eq!(wake.dedupe_key(), format!("{effect_id}:0"));
    // a retried attempt must arm its own timer rather than collide with the one it replaced.
    let mut retried = wake.clone();
    retried.result.attempt = 1;
    assert_ne!(wake.dedupe_key(), retried.dedupe_key());
}

#[test]
fn orchestration_wakes_round_trip_without_effect_semantics() {
    let binding_id = Uuid::now_v7();
    let due_at = chrono::Utc::now() + chrono::Duration::seconds(30);
    let wake =
        WakeCommand::orchestration_intent(due_at, binding_id, "scope_changed", Uuid::now_v7());
    let decoded = WakeCommand::from_wire(&wake.to_wire().unwrap()).unwrap();
    let intent = decoded.orchestration_intent.as_ref().unwrap();
    assert_eq!(intent.binding_id, binding_id);
    assert_eq!(intent.intent, "scope_changed");
    assert_eq!(decoded.dedupe_key(), wake.dedupe_key());

    let ingress =
        WsIngressCommand::orchestration_intent(intent.clone(), decoded.due_at, decoded.trace_id);
    let round_trip = WsIngressCommand::from_wire(&ingress.to_wire().unwrap()).unwrap();
    assert_eq!(round_trip.dedupe_key(), ingress.dedupe_key());
    assert!(matches!(
        round_trip,
        WsIngressCommand::OrchestrationIntent { wake, .. }
            if wake.binding_id == binding_id && wake.intent == "scope_changed"
    ));
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
fn terminal_control_round_trips_with_effect_and_replica_routing() {
    use runinator_models::runs::ProviderTerminalControl;

    let workflow_run_id = Uuid::now_v7();
    let effect_id = Uuid::now_v7();
    let replica_id = Uuid::now_v7();
    let command = ControlCommand::for_terminal(
        workflow_run_id,
        effect_id,
        ProviderTerminalControl::Input {
            data: "hello\r".into(),
        },
    )
    .targeting_replica(replica_id);
    let decoded = ControlCommand::from_wire(&command.to_wire().unwrap()).unwrap();

    assert_eq!(decoded.kind, ControlKind::Terminal);
    assert_eq!(decoded.effect_id, Some(effect_id));
    assert_eq!(decoded.target, ActionTarget::Replica { replica_id });
    assert_eq!(decoded.terminal, command.terminal);
}

#[test]
fn effect_results_round_trip_with_json() {
    let command = EffectCommand {
        version: runinator_models::workflow_vm::WORKFLOW_EFFECT_PROTOCOL_VERSION,
        command_id: Uuid::now_v7(),
        effect_id: Uuid::now_v7(),
        workflow_run_id: Uuid::now_v7(),
        continuation_id: Uuid::now_v7(),
        attempt: 1,
        request: runinator_models::workflow_vm::WorkflowEffectRequest::Timer { due_at: 1 },
        executor: EffectExecutor::Provider,
        target: Default::default(),
        trace_id: Uuid::nil(),
        trace_context: Default::default(),
        idempotency_key: "effect-key".into(),
        notification_delivery_id: None,
    };
    let result = EffectResult::status(
        &command,
        runinator_models::workflow_vm::WorkflowEffectStatus::Succeeded,
        Some(json!({"ok": true})),
        None,
    );

    let encoded = result.to_wire().unwrap();
    let decoded = EffectResult::from_wire(&encoded).unwrap();

    assert_eq!(decoded.effect_id, command.effect_id);
    assert_eq!(decoded.continuation_id, command.continuation_id);
    assert_eq!(decoded.attempt, 1);
    match decoded.kind {
        EffectResultKind::Status { status, output, .. } => {
            assert_eq!(
                status,
                runinator_models::workflow_vm::WorkflowEffectStatus::Succeeded
            );
            assert_eq!(output, Some(json!({"ok": true})));
        }
        _ => panic!("expected status result"),
    }

    // a message from an unknown future protocol version decodes, then fails the version gate
    // rather than being silently applied.
    let mut future = serde_json::to_value(&result).unwrap();
    future.as_object_mut().unwrap().insert(
        "version".into(),
        serde_json::Value::from(
            runinator_models::workflow_vm::WORKFLOW_EFFECT_PROTOCOL_VERSION + 1,
        ),
    );
    let decoded: EffectResult = serde_json::from_value(future).unwrap();
    assert!(!decoded.is_supported());
}

#[test]
fn terminal_interaction_results_round_trip_with_json() {
    let result = EffectResult {
        workspace_commit: None,
        version: runinator_models::workflow_vm::WORKFLOW_EFFECT_PROTOCOL_VERSION,
        event_id: Uuid::now_v7(),
        effect_id: Uuid::now_v7(),
        workflow_run_id: Uuid::now_v7(),
        continuation_id: Uuid::now_v7(),
        attempt: 2,
        kind: EffectResultKind::TerminalInteraction {
            interaction: runinator_models::runs::TerminalInteraction {
                sequence: 7,
                request_id: "otp".into(),
                state: runinator_models::runs::TerminalInteractionState::InputRequired,
                prompt: Some("One-time code".into()),
            },
        },
        timestamp: chrono::Utc::now(),
        trace_id: Uuid::now_v7(),
        notification_delivery_id: None,
    };
    let decoded = EffectResult::from_wire(&result.to_wire().unwrap()).unwrap();
    assert!(matches!(
        decoded.kind,
        EffectResultKind::TerminalInteraction { interaction }
            if interaction.sequence == 7 && interaction.request_id == "otp"
    ));
}

#[test]
fn ui_event_round_trips_org_scope_and_accepts_legacy_unscoped_json() {
    let run_id = Uuid::now_v7();
    let org_id = Uuid::now_v7();
    let scoped = UiEvent::for_org(org_id, UiEventKind::WorkflowRunChanged { run_id });
    let value = serde_json::to_value(&scoped).unwrap();
    assert_eq!(value["type"], "workflow_run_changed");
    assert_eq!(value["run_id"], run_id.to_string());
    assert_eq!(value["org_id"], org_id.to_string());

    let decoded: UiEvent = serde_json::from_value(value).unwrap();
    assert_eq!(decoded.org_id, Some(org_id));
    assert!(matches!(
        decoded.kind,
        UiEventKind::WorkflowRunChanged { run_id: id } if id == run_id
    ));

    // pre-scope publishers omit org_id; they must remain deliverable as unscoped.
    let legacy = r#"{"type":"workflows_changed"}"#.to_string();
    let decoded: UiEvent = serde_json::from_str(&legacy).unwrap();
    assert_eq!(decoded.org_id, None);
    assert!(matches!(decoded.kind, UiEventKind::WorkflowsChanged));
}

#[test]
fn orchestration_ui_events_keep_target_identifiers() {
    let orchestration_id = Uuid::now_v7();
    let operation_id = Uuid::now_v7();
    let adapter_id = Uuid::now_v7();
    let events = [
        UiEventKind::OrchestrationChanged { orchestration_id },
        UiEventKind::ExternalOperationChanged {
            operation_id,
            orchestration_id,
        },
        UiEventKind::AdapterChanged { adapter_id },
    ];

    let values = events
        .into_iter()
        .map(|kind| serde_json::to_value(UiEvent::global(kind)).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(values[0]["type"], "orchestration_changed");
    assert_eq!(values[0]["orchestration_id"], orchestration_id.to_string());
    assert_eq!(values[1]["type"], "external_operation_changed");
    assert_eq!(values[1]["operation_id"], operation_id.to_string());
    assert_eq!(values[2]["type"], "adapter_changed");
    assert_eq!(values[2]["adapter_id"], adapter_id.to_string());
}

#[test]
fn legacy_web_service_announcements_get_safe_defaults() {
    let service_id = Uuid::now_v7();
    let raw = format!(
        r#"{{"service_id":"{service_id}","address":"127.0.0.1","port":8080,"last_heartbeat":"{}"}}"#,
        Utc::now().to_rfc3339()
    );
    let decoded: WebServiceAnnouncement = serde_json::from_str(&raw).unwrap();
    assert_eq!(decoded.scheme, "http");
    assert_eq!(decoded.relay_path, "/ws/broker");
    assert_eq!(decoded.cluster_id, Uuid::nil());
    assert!(!decoded.enrollment_enabled);
}

#[tokio::test]
async fn discovery_selects_only_the_token_bound_cluster() {
    let discovery = crate::discovery::WebServiceDiscovery::new();
    let expected_cluster = Uuid::now_v7();
    for (cluster_id, address) in [
        (Uuid::now_v7(), "attacker.local"),
        (expected_cluster, "trusted.local"),
    ] {
        discovery
            .register(WebServiceAnnouncement {
                service_id: Uuid::now_v7(),
                address: address.into(),
                port: 8443,
                base_path: Some("/api/".into()),
                scheme: "https".into(),
                relay_path: "/relay".into(),
                cluster_id,
                enrollment_enabled: true,
                spki_pin: None,
                version: Some("1.0.0".into()),
                last_heartbeat: Utc::now(),
            })
            .await;
    }
    let selected = discovery
        .current_service_for_cluster(expected_cluster)
        .await
        .unwrap();
    assert_eq!(selected.address, "trusted.local");
    assert_eq!(
        crate::discovery::web_service_base_url(&selected),
        "https://trusted.local:8443/api/"
    );
}

#[tokio::test]
async fn virtual_udp_broadcast_drives_the_real_discovery_listener() {
    use std::net::{Ipv4Addr, SocketAddr};

    let net = crate::discovery::VirtualNet::default();
    let listener_socket = net.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 5000)));
    let sender = net.bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 5001)));
    let discovery = crate::discovery::spawn_web_service_listener_with_socket(listener_socket);
    let cluster_id = Uuid::now_v7();
    let message = GossipMessage::WebService {
        service: WebServiceAnnouncement {
            service_id: Uuid::now_v7(),
            address: "lan-cluster.local".into(),
            port: 8443,
            base_path: Some("/".into()),
            scheme: "https".into(),
            relay_path: "/ws/agent".into(),
            cluster_id,
            enrollment_enabled: true,
            spki_pin: None,
            version: None,
            last_heartbeat: Utc::now(),
        },
    };
    crate::discovery::broadcast_gossip_message(
        sender.as_ref(),
        &message,
        &["255.255.255.255:5000".into()],
    )
    .await;
    let url = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        discovery.wait_for_cluster_url(cluster_id),
    )
    .await
    .unwrap();
    assert_eq!(url, "https://lan-cluster.local:8443/");
}
