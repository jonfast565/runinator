use crate::{
    ActionCommand, ActionTarget, AgentDirectiveKind, ControlCommand, ControlKind, GossipMessage,
    UiEvent, UiEventKind, WakeCommand, WebServiceAnnouncement, WireCodec, WorkflowResultEvent,
    WorkflowResultEventKind, WsIngressCommand,
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
fn unknown_agent_directive_types_deserialize_as_unsupported_candidates() {
    let kind: AgentDirectiveKind =
        serde_json::from_str(r#"{ "type": "future_operation", "new_field": true }"#).unwrap();
    assert!(matches!(kind, AgentDirectiveKind::Unknown));
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
            idempotency_key: None,
            function_binding: None,
        },
        attempt: 1,
        parameters: json!({}),
        target: Default::default(),
        trace_id: Uuid::nil(),
        trace_context: Default::default(),
        notification_delivery_id: None,
        invocation_call_id: None,
        idempotency_key: None,
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
fn legacy_web_service_announcements_get_safe_defaults() {
    let service_id = Uuid::now_v7();
    let raw = format!(
        r#"{{"service_id":"{service_id}","address":"127.0.0.1","port":8080,"last_heartbeat":"{}"}}"#,
        Utc::now().to_rfc3339()
    );
    let decoded: WebServiceAnnouncement = serde_json::from_str(&raw).unwrap();
    assert_eq!(decoded.scheme, "http");
    assert_eq!(decoded.relay_path, "/ws/desktop-worker");
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
