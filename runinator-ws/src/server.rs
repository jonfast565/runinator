use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};

use log::info;
use runinator_broker::Broker;
use runinator_database::{initialize_database, interfaces::DatabaseImpl};
use runinator_models::errors::SendableError;
use runinator_models::replicas::{
    ReplicaHeartbeatRequest, ReplicaKind, ReplicaRegistrationRequest,
};
use tokio::{
    net::TcpListener,
    sync::{Notify, broadcast},
};
use uuid::Uuid;

use crate::background::{
    instance_id, run_action_dispatch_publisher, run_event_consumer, run_ingress_consumer,
    run_replica_reaper, run_trigger_loop, run_wake_publisher,
};
use crate::events::{AppEvent, EventBus};
use crate::handlers::catalog::seed_builtin_catalog;
use crate::result_consumer::run_result_consumer;
use crate::router::build_router;

/// what this web service replica advertises to the replica list at registration and on every
/// heartbeat. host is its stable dns name; attributes carry the broker/database backend it runs on.
#[derive(Debug, Clone, Default)]
pub struct ReplicaAdvertisement {
    pub host: Option<String>,
    pub attributes: runinator_models::value::Value,
}

pub async fn run_webserver<T: DatabaseImpl>(
    pool: Arc<T>,
    notify: Arc<Notify>,
    port: u16,
    broker: Arc<dyn Broker>,
    advertisement: ReplicaAdvertisement,
) -> Result<(), SendableError> {
    initialize_database(&pool).await?;
    seed_builtin_catalog(pool.as_ref()).await?;
    let (events_tx, _) = broadcast::channel::<AppEvent>(1024);
    let instance = instance_id();
    let runtime_id = Uuid::new_v4().to_string();
    let web_replica = crate::repository::register_replica(
        pool.as_ref(),
        ReplicaRegistrationRequest {
            replica_type: ReplicaKind::Webservice,
            instance_id: instance.clone(),
            runtime_id: runtime_id.clone(),
            display_name: Some(instance.clone()),
            host: advertisement.host.clone(),
            port: Some(port),
            base_path: Some("/".into()),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            attributes: advertisement.attributes.clone(),
        },
        None,
    )
    .await?;
    let heartbeat_db = pool.clone();
    let heartbeat_notify = notify.clone();
    let heartbeat_runtime_id = runtime_id.clone();
    let heartbeat_instance = instance.clone();
    let heartbeat_host = advertisement.host.clone();
    let heartbeat_attributes = advertisement.attributes.clone();
    let heartbeat = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(10));
        loop {
            tokio::select! {
                _ = heartbeat_notify.notified() => {
                    let _ = crate::repository::mark_replica_offline(
                        heartbeat_db.as_ref(),
                        web_replica.replica_id,
                        heartbeat_runtime_id.clone(),
                    ).await;
                    return;
                }
                _ = ticker.tick() => {
                    let _ = crate::repository::heartbeat_replica(
                        heartbeat_db.as_ref(),
                        web_replica.replica_id,
                        ReplicaHeartbeatRequest {
                            runtime_id: heartbeat_runtime_id.clone(),
                            display_name: Some(heartbeat_instance.clone()),
                            host: heartbeat_host.clone(),
                            port: Some(port),
                            base_path: Some("/".into()),
                            attributes: heartbeat_attributes.clone(),
                        },
                        None,
                    ).await;
                }
            }
        }
    });
    // the bus publishes emitted events to the broker; the event consumer is the sole writer to the
    // local broadcast that feeds this replica's WebSocket clients.
    let bus = EventBus::new(events_tx.clone(), broker.clone());
    let background = vec![
        heartbeat,
        tokio::spawn(run_result_consumer(
            pool.clone(),
            broker.clone(),
            bus.clone(),
            notify.clone(),
        )),
        tokio::spawn(run_ingress_consumer(
            pool.clone(),
            broker.clone(),
            bus.clone(),
            instance.clone(),
            notify.clone(),
        )),
        tokio::spawn(run_wake_publisher(
            pool.clone(),
            broker.clone(),
            notify.clone(),
        )),
        tokio::spawn(run_trigger_loop(
            pool.clone(),
            bus.clone(),
            instance.clone(),
            notify.clone(),
        )),
        tokio::spawn(run_action_dispatch_publisher(
            pool.clone(),
            broker.clone(),
            instance.clone(),
            notify.clone(),
        )),
        tokio::spawn(run_event_consumer(
            broker.clone(),
            events_tx.clone(),
            instance.clone(),
            notify.clone(),
        )),
        tokio::spawn(run_replica_reaper(pool.clone(), notify.clone())),
    ];
    let app = build_router(pool, bus, broker);
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr).await?;
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    );
    info!("Webserver started at {}:{}", addr.ip(), addr.port());

    tokio::select! {
        result = server => {
            abort_all(&background);
            if let Err(err) = result {
                log::error!("Webserver error: {}", err);
                return Err(Box::new(err));
            }
            Ok(())
        }
        _ = notify.notified() => {
            info!("Shutting down web server...");
            abort_all(&background);
            Ok(())
        }
    }
}

fn abort_all(handles: &[tokio::task::JoinHandle<()>]) {
    for handle in handles {
        handle.abort();
    }
}
