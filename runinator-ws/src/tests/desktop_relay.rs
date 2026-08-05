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
    assert!(err.to_string().contains("exclusive"));

    // policy: an exclusive profile presenting a replica_id nobody registered is refused.
    let unknown_replica = ConsumerProfile::shared("desktop")
        .with_replica_id(Uuid::now_v7())
        .exclusive();
    let err = client
        .receive_for(&unknown_replica)
        .await
        .expect_err("an unregistered replica_id must be refused");
    assert!(err.to_string().contains("unknown replica_id"));

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
    assert!(err.to_string().contains("not permitted"));

    // allow-list: control-channel receive/ack is permitted (a desktop worker needs it for cancel).
    inner_broker
        .publish_control(ControlCommand::new(
            Uuid::now_v7(),
            runinator_comm::ControlKind::Cancel,
        ))
        .await
        .unwrap();
    let control_delivery = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.receive_control("desktop-control"),
    )
    .await
    .unwrap()
    .expect("receive_control must be permitted over the desktop-worker relay");
    client
        .ack_control("desktop-control", control_delivery.delivery_id)
        .await
        .expect("ack_control must be permitted over the desktop-worker relay");

    server.abort();
    let _ = std::fs::remove_file(path);
}
