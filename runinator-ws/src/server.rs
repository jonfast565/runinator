use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};

use log::info;
use runinator_broker::Broker;
use runinator_database::{initialize_database, interfaces::DatabaseImpl};
use runinator_models::errors::SendableError;
use tokio::{
    net::TcpListener,
    sync::{Notify, broadcast},
};

use crate::background::{
    instance_id, run_action_dispatch_publisher, run_event_consumer, run_ingress_consumer,
    run_trigger_loop, run_wake_publisher,
};
use crate::events::{AppEvent, EventBus};
use crate::handlers::catalog::seed_builtin_catalog;
use crate::result_consumer::run_result_consumer;
use crate::router::build_router;

pub async fn run_webserver<T: DatabaseImpl>(
    pool: Arc<T>,
    notify: Arc<Notify>,
    port: u16,
    broker: Arc<dyn Broker>,
) -> Result<(), SendableError> {
    initialize_database(&pool).await?;
    seed_builtin_catalog(pool.as_ref()).await?;
    let (events_tx, _) = broadcast::channel::<AppEvent>(1024);
    let instance = instance_id();
    // the bus publishes emitted events to the broker; the event consumer is the sole writer to the
    // local broadcast that feeds this replica's WebSocket clients.
    let bus = EventBus::new(events_tx.clone(), broker.clone());
    let background = vec![
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
    ];
    let app = build_router(pool, bus, broker);
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr).await?;
    let server = axum::serve(listener, app);
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
