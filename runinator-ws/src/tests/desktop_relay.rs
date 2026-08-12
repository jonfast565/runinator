//! the `/ws/desktop-worker` broker relay: channel policy allow-list and replica ownership.
//!
//! the module is gated on the `ws` feature, which is declared in `mod.rs`: the real `WsBroker`
//! client needs `runinator-broker`'s `ws` feature, and without it every call returns
//! `FeatureDisabled` immediately, which isn't what this exercises. run with `--features ws`.

use super::*;

/// end-to-end coverage for the relay against a minimal router carrying just that one route (plus its
/// `Extension`s) rather than the full `build_router` stack, since the policy allow-list and
/// replica-ownership check are this handler's own logic, independent of auth middleware/CORS/
/// rate-limiting already covered elsewhere.
#[tokio::test]
async fn ws_desktop_worker_relay_enforces_policy_and_ownership() {
    use axum::{Router, routing::get};
    use runinator_broker::ConsumerProfile;
    use runinator_broker::ws::client::WsBroker;
    use runinator_comm::ActionTarget;
    use runinator_models::replicas::{ReplicaKind, ReplicaRegistrationRequest};
    use tokio::net::TcpListener;

    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let inner_broker = Arc::new(InMemoryBroker::new());
    let broker_ext: Arc<dyn Broker> = inner_broker.clone();
    let ctx = AuthContext {
        principal_id: Some(Uuid::now_v7()),
        is_admin: false,
        kind: PrincipalKind::Service,
        org_id: None,
        org_role: None,
    };

    let router = Router::new()
        .route(
            "/ws/desktop-worker",
            get(crate::websocket::ws_desktop_worker::<SqliteDb>),
        )
        .layer(Extension(db.clone()))
        .layer(Extension(broker_ext))
        .layer(Extension(ctx.clone()));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    let client = WsBroker::connect(format!("ws://{addr}/ws/desktop-worker"), None);

    // policy: a non-exclusive profile is refused outright, regardless of replica_id/labels.
    let non_exclusive = ConsumerProfile::shared("cloud-worker");
    let err = client
        .receive_for(&non_exclusive)
        .await
        .expect_err("a non-exclusive profile must be refused");
    // asserted on the dictionary code, not the prose: the code is the part an agent surfaces to its
    // operator and the part that stays stable if the wording is ever reworded.
    assert!(err.to_string().contains("RUNI177"), "got {err}");

    // policy: an exclusive profile presenting a replica_id nobody registered is refused.
    let unknown_replica = ConsumerProfile::shared("desktop")
        .with_replica_id(Uuid::now_v7())
        .exclusive();
    let err = client
        .receive_for(&unknown_replica)
        .await
        .expect_err("an unregistered replica_id must be refused");
    assert!(err.to_string().contains("RUNI180"), "got {err}");

    // ownership: register a replica under `ctx`'s principal, then presenting that replica_id from
    // the same principal must be accepted (dispatched through to the real broker underneath).
    let registration = db
        .register_replica(
            ReplicaRegistrationRequest {
                replica_type: ReplicaKind::Worker,
                instance_id: "desktop-test".into(),
                runtime_id: Uuid::new_v4().to_string(),
                display_name: None,
                host: None,
                port: None,
                base_path: None,
                version: None,
                attributes: runinator_models::json!({}),
            },
            None,
            &ctx,
        )
        .await
        .unwrap();
    let owned_profile = ConsumerProfile::shared("desktop")
        .with_replica_id(registration.replica_id)
        .exclusive();
    inner_broker
        .publish(BrokerMessage {
            command: {
                let mut command =
                    action_command(Uuid::now_v7(), Uuid::now_v7(), "relay-ownership-node");
                command.target = ActionTarget::Replica {
                    replica_id: registration.replica_id,
                };
                command
            },
            dedupe_key: Some("relay-ownership-test".into()),
            enqueued_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let delivery = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.receive_for(&owned_profile),
    )
    .await
    .unwrap()
    .expect("the owning principal must be able to receive its own replica's targeted action");
    client
        .ack(&owned_profile.id, delivery.delivery_id)
        .await
        .unwrap();

    // policy: a disallowed op (publishing a general action) is refused even though the connection
    // and the broker underneath both work fine, per the two checks above.
    let err = client
        .publish(BrokerMessage {
            command: action_command(Uuid::now_v7(), Uuid::now_v7(), "relay-disallowed-node"),
            dedupe_key: Some("relay-disallowed-test".into()),
            enqueued_at: chrono::Utc::now(),
        })
        .await
        .expect_err("publish must not be permitted over the desktop-worker relay");
    assert!(err.to_string().contains("RUNI178"), "got {err}");
    // the refusal names the operation, so an operator reading an agent's log can tell *which* call
    // was refused rather than only that something was.
    assert!(err.to_string().contains("publish"), "got {err}");

    // allow-list: targeted control receive/ack is permitted; plain ReceiveControl is deliberately
    // refused because an exclusive relay must not consume an ActionTarget::Any run-wide control.
    inner_broker
        .publish_control(
            ControlCommand::new(Uuid::now_v7(), runinator_comm::ControlKind::Cancel)
                .targeting_replica(registration.replica_id),
        )
        .await
        .unwrap();
    let control_delivery = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.receive_control_for(&owned_profile),
    )
    .await
    .unwrap()
    .expect("receive_control must be permitted over the desktop-worker relay");
    client
        .ack_control(&owned_profile.id, control_delivery.delivery_id)
        .await
        .expect("ack_control must be permitted over the desktop-worker relay");

    let plain_control = client
        .receive_control("desktop-control")
        .await
        .expect_err("plain control receive must be refused over an exclusive relay");
    assert!(plain_control.to_string().contains("RUNI178"));

    // agent directives are the fourth receive/ack channel and remain replica-owned.
    let directive_id = Uuid::now_v7();
    inner_broker
        .publish_agent(runinator_comm::AgentCommand {
            directive_id,
            replica_id: registration.replica_id,
            target: ActionTarget::Replica {
                replica_id: registration.replica_id,
            },
            kind: runinator_comm::AgentDirectiveKind::Diagnostics,
            issued_at: chrono::Utc::now(),
            expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
        })
        .await
        .unwrap();
    let agent_delivery = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.receive_agent_for(&owned_profile),
    )
    .await
    .unwrap()
    .expect("owned agent directive receive must be permitted");
    client
        .ack_agent(&owned_profile.id, agent_delivery.delivery_id)
        .await
        .expect("agent directive ack must be permitted");

    let unowned_agent = client
        .receive_agent_for(&unknown_replica)
        .await
        .expect_err("agent directives for an unowned replica must be refused");
    assert!(unowned_agent.to_string().contains("RUNI180"));

    // ingress is payload-gated: directive results pass, reducer drives do not.
    client
        .publish_ingress(runinator_broker::IngressMessage {
            command: runinator_comm::WsIngressCommand::AgentDirectiveResult {
                result: runinator_comm::AgentDirectiveResult {
                    directive_id,
                    status: runinator_comm::AgentDirectiveStatus::Completed,
                    payload: runinator_models::json!({ "ok": true }),
                    message: None,
                },
            },
            dedupe_key: None,
            enqueued_at: chrono::Utc::now(),
        })
        .await
        .expect("agent directive results may publish to ingress");
    let drive_err = client
        .publish_ingress(runinator_broker::IngressMessage {
            command: runinator_comm::WsIngressCommand::drive(
                Uuid::now_v7(),
                Uuid::now_v7(),
                "forbidden".to_string(),
                Uuid::now_v7(),
            ),
            dedupe_key: None,
            enqueued_at: chrono::Utc::now(),
        })
        .await
        .expect_err("general ingress publication must remain refused");
    assert!(drive_err.to_string().contains("RUNI178"));

    server.abort();
    let _ = std::fs::remove_file(path);
}

/// a delivery the relay takes off the broker but cannot forward must go back on the broker.
///
/// this is the window unique to the relay: `dispatch` pulls a delivery, and the socket can die
/// before the reply carrying it reaches the agent. the agent never saw it and so will never ack it,
/// which used to leave it leased to a consumer that no longer exists — stalling that work until the
/// lease expired. the connection is severed through a one-shot proxy rather than by dropping the
/// client, because `WsBroker` detaches its supervisor task and dropping it does not close the socket.
#[tokio::test]
async fn relay_returns_a_delivery_it_could_not_forward() {
    use axum::{Router, routing::get};
    use runinator_broker::ConsumerProfile;
    use runinator_broker::ws::client::WsBroker;
    use runinator_comm::ActionTarget;
    use runinator_models::replicas::{ReplicaKind, ReplicaRegistrationRequest};
    use tokio::net::TcpListener;

    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let inner_broker = Arc::new(InMemoryBroker::new());
    let broker_ext: Arc<dyn Broker> = inner_broker.clone();
    let ctx = AuthContext {
        principal_id: Some(Uuid::now_v7()),
        is_admin: false,
        kind: PrincipalKind::Service,
        org_id: None,
        org_role: None,
    };

    let router = Router::new()
        .route(
            "/ws/desktop-worker",
            get(crate::websocket::ws_desktop_worker::<SqliteDb>),
        )
        .layer(Extension(db.clone()))
        .layer(Extension(broker_ext))
        .layer(Extension(ctx.clone()));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay_addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    let registration = db
        .register_replica(
            ReplicaRegistrationRequest {
                replica_type: ReplicaKind::Worker,
                instance_id: "stranded-test".into(),
                runtime_id: Uuid::new_v4().to_string(),
                display_name: None,
                host: None,
                port: None,
                base_path: None,
                version: None,
                attributes: runinator_models::json!({}),
            },
            None,
            &ctx,
        )
        .await
        .unwrap();
    let profile = ConsumerProfile::shared("stranded-desktop")
        .with_replica_id(registration.replica_id)
        .exclusive();

    // a proxy that carries exactly one connection and can be cut on demand.
    let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy.local_addr().unwrap();
    let (cut_tx, cut_rx) = tokio::sync::oneshot::channel::<()>();
    let proxy_task = tokio::spawn(async move {
        let (mut downstream, _) = proxy.accept().await.unwrap();
        let mut upstream = tokio::net::TcpStream::connect(relay_addr).await.unwrap();
        tokio::select! {
            _ = tokio::io::copy_bidirectional(&mut downstream, &mut upstream) => {}
            _ = cut_rx => {}
        }
        // both halves drop here, closing the relay's socket.
    });

    let client = WsBroker::connect(format!("ws://{proxy_addr}/ws/desktop-worker"), None);

    // park a `receive_for` on the relay. detached: it retries across reconnects forever by contract,
    // and after the cut there is nothing left to reconnect to.
    let parked_profile = profile.clone();
    let parked = tokio::spawn(async move { client.receive_for(&parked_profile).await });

    // let the request reach the relay and block in the broker before cutting the connection.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let _ = cut_tx.send(());
    let _ = proxy_task.await;

    // now hand the parked consumer a delivery it cannot possibly forward.
    let mut command = action_command(Uuid::now_v7(), Uuid::now_v7(), "stranded-node");
    command.target = ActionTarget::Replica {
        replica_id: registration.replica_id,
    };
    inner_broker
        .publish(BrokerMessage {
            command,
            dedupe_key: Some("stranded-delivery".into()),
            enqueued_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    // a fresh agent must still be able to pick it up. without the nack it stays leased to the
    // consumer behind the dead socket and this receive hangs until the test's timeout.
    let recovered = WsBroker::connect(format!("ws://{relay_addr}/ws/desktop-worker"), None);
    let delivery = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        recovered.receive_for(&profile),
    )
    .await
    .expect("the undelivered action must be returned to the broker, not stranded")
    .expect("receive_for must succeed on the reconnected client");
    assert_eq!(delivery.command.node_id, "stranded-node");

    parked.abort();
    server.abort();
    let _ = std::fs::remove_file(path);
}
