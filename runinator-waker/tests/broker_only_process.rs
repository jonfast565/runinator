use std::{
    net::SocketAddr,
    process::{Command, Stdio},
    time::Duration,
};

use chrono::Utc;
use runinator_broker::{
    Broker, EffectResult, EffectResultKind, WakeCommand, WakeMessage, WsIngressCommand,
    in_memory::InMemoryBroker,
};
use runinator_models::workflow_vm::{WORKFLOW_EFFECT_PROTOCOL_VERSION, WorkflowEffectStatus};
use uuid::Uuid;

struct ChildGuard(std::process::Child);

impl ChildGuard {
    fn new(child: std::process::Child) -> Self {
        Self(child)
    }

    fn child_mut(&mut self) -> &mut std::process::Child {
        &mut self.0
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn due_result(due_at: chrono::DateTime<Utc>) -> EffectResult {
    EffectResult {
        version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
        event_id: Uuid::now_v7(),
        effect_id: Uuid::now_v7(),
        workflow_run_id: Uuid::now_v7(),
        continuation_id: Uuid::now_v7(),
        attempt: 0,
        kind: EffectResultKind::Status {
            status: WorkflowEffectStatus::Succeeded,
            output: None,
            message: None,
        },
        timestamp: due_at,
        trace_id: Uuid::now_v7(),
        notification_delivery_id: None,
    }
}

#[tokio::test]
async fn broker_only_process_relays_a_due_wake_without_a_web_service() {
    let broker = InMemoryBroker::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint: SocketAddr = listener.local_addr().unwrap();
    let broker_server = tokio::spawn(runinator_broker::tcp::server::serve(
        listener,
        broker.clone(),
    ));

    // No web service is started. An API key is deliberately present to make sure it is inert for
    // this broker-only process; the wake and ingress channels are its entire external contract.
    let mut waker = ChildGuard::new(
        Command::new(env!("CARGO_BIN_EXE_runinator-waker"))
            .args([
                "--broker-backend",
                "tcp",
                "--broker-endpoint",
                &endpoint.to_string(),
                "--broker-client-id",
                "broker-only-process-test",
                "--waker-consumer-group",
                "broker-only-process-test",
                "--liveness-file",
                "",
            ])
            .env("RUNINATOR_API_KEY", "not-used-by-the-waker")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("waker process should start from broker configuration alone"),
    );

    let due_at = Utc::now() - chrono::Duration::seconds(1);
    let result = due_result(due_at);
    broker
        .publish_wake(WakeMessage {
            command: WakeCommand::new(due_at, result.clone(), Uuid::now_v7()),
            dedupe_key: None,
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();

    // The waker first announces its broker availability. A deployment normally has an engine
    // consuming that observation; this broker-only test owns ingress itself, so acknowledge it
    // and keep waiting for the wake settlement it exists to prove.
    loop {
        let delivery = tokio::time::timeout(
            Duration::from_secs(10),
            broker.receive_ingress("broker-only-process-test"),
        )
        .await
        .expect("waker should settle the due wake without waiting for a web service")
        .unwrap();
        match delivery.command {
            WsIngressCommand::SettleEffect {
                result: relayed, ..
            } => {
                assert_eq!(relayed.effect_id, result.effect_id);
                broker
                    .ack_ingress("broker-only-process-test", delivery.delivery_id)
                    .await
                    .unwrap();
                break;
            }
            WsIngressCommand::ReplicaAvailability { .. } => {
                broker
                    .ack_ingress("broker-only-process-test", delivery.delivery_id)
                    .await
                    .unwrap();
            }
            other => panic!("expected a settle effect, got {other:?}"),
        }
    }

    assert!(
        waker.child_mut().try_wait().unwrap().is_none(),
        "waker should remain alive after relaying a wake"
    );
    broker_server.abort();
}
