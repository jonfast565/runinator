use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};

use runinator_broker::Broker;
use runinator_database::{interfaces::DatabaseImpl, load_jwt_secret, load_jwt_secret_previous};
use runinator_models::auth::AuthContext;
use runinator_models::errors::SendableError;
use runinator_models::replicas::{
    ReplicaHeartbeatRequest, ReplicaKind, ReplicaRegistrationRequest,
};
use tokio::{
    net::TcpListener,
    sync::{Notify, broadcast},
    task::JoinSet,
};
use tracing::{error, info, warn};
use uuid::Uuid;

use runinator_engine::{EnginePublisher, run_background_engine};

use crate::event_consumer::{instance_id, run_event_consumer};
use crate::events::{AppEvent, EventBus};
use crate::handlers::catalog::seed_builtin_catalog;
use crate::router::build_router;

/// what this web service replica advertises to the replica list at registration and on every
/// heartbeat. host is its stable dns name; attributes carry the broker/database backend it runs on.
#[derive(Debug, Clone, Default)]
pub struct ReplicaAdvertisement {
    pub instance_id: Option<String>,
    pub host: Option<String>,
    pub attributes: runinator_models::value::Value,
}

pub async fn run_webserver<T: DatabaseImpl>(
    pool: Arc<T>,
    notify: Arc<Notify>,
    port: u16,
    broker: Arc<dyn Broker>,
    advertisement: ReplicaAdvertisement,
    auth: crate::auth::AuthOptions,
    rate_limit: crate::rate_limit::RateLimitConfig,
    overload: crate::overload::OverloadConfig,
    run_engine: bool,
) -> Result<(), SendableError> {
    crate::stability::init_metrics();
    seed_builtin_catalog(pool.as_ref()).await?;
    let jwt_secret = load_jwt_secret(pool.as_ref()).await?;
    let jwt_secret_previous = load_jwt_secret_previous(pool.as_ref()).await?;
    if auth.enabled {
        info!("HTTP API authentication is ENABLED");
    } else {
        warn!("HTTP API authentication is DISABLED");
    }
    if jwt_secret_previous.is_some() {
        info!("accepting a previous jwt signing secret (key rotation overlap window is open)");
    }
    let auth_config = crate::auth::AuthConfig {
        enabled: auth.enabled,
        jwt_secret,
        jwt_secret_previous,
        access_ttl_secs: auth.access_ttl_secs,
        refresh_ttl_secs: auth.refresh_ttl_secs,
    };
    let (events_tx, _) = broadcast::channel::<AppEvent>(1024);
    let instance = advertisement
        .instance_id
        .clone()
        .unwrap_or_else(instance_id);
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
            attributes: runinator_utilities::resource_telemetry::attributes_with_host_metadata(
                &advertisement.attributes,
            ),
        },
        None,
        // the web service registering its own replica at startup, not an external caller.
        &AuthContext::disabled_admin(),
    )
    .await?;
    let heartbeat_db = pool.clone();
    let heartbeat_notify = notify.clone();
    let heartbeat_runtime_id = runtime_id.clone();
    let heartbeat_instance = instance.clone();
    let heartbeat_host = advertisement.host.clone();
    let heartbeat_attributes = advertisement.attributes.clone();
    let heartbeat_telemetry =
        std::sync::Arc::new(runinator_utilities::resource_telemetry::TelemetryCollector::new());
    // every long-lived loop runs in this set so an unexpected exit (panic or early return) is
    // observed at the join below instead of silently leaving a dead loop behind.
    let mut background: JoinSet<()> = JoinSet::new();
    background.spawn(async move {
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
                    let attributes = runinator_utilities::resource_telemetry::attributes_with_telemetry(
                        &heartbeat_attributes,
                        heartbeat_telemetry.as_ref(),
                    );
                    let _ = crate::repository::heartbeat_replica(
                        heartbeat_db.as_ref(),
                        web_replica.replica_id,
                        ReplicaHeartbeatRequest {
                            runtime_id: heartbeat_runtime_id.clone(),
                            display_name: Some(heartbeat_instance.clone()),
                            host: heartbeat_host.clone(),
                            port: Some(port),
                            base_path: Some("/".into()),
                            attributes,
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
    // every replica consumes the broker fan-out events channel so its WebSocket clients see events
    // emitted by any replica or a standalone background worker, regardless of who did the work.
    background.spawn(run_event_consumer(
        broker.clone(),
        events_tx.clone(),
        instance.clone(),
        notify.clone(),
    ));
    // run the durable orchestration engine in-process unless a standalone background worker owns it.
    // the engine publishes UI events onto the broker; this replica's event consumer above fans them
    // out to WebSocket clients either way.
    if run_engine {
        info!("embedding the background orchestration engine in-process");
        let engine_pool = pool.clone();
        let engine_broker = broker.clone();
        let engine_publisher = EnginePublisher::new(broker.clone());
        let engine_instance = instance.clone();
        let engine_shutdown = notify.clone();
        background.spawn(async move {
            // run_background_engine drives its own shutdown on internal loop failure, so a returned
            // Err has already notified `notify`; returning here surfaces the exit to the join below.
            if let Err(err) = run_background_engine(
                engine_pool,
                engine_broker,
                engine_publisher,
                engine_instance,
                engine_shutdown,
            )
            .await
            {
                error!("in-process background engine exited: {err}");
            }
        });
    } else {
        info!(
            "background orchestration engine is DISABLED in-process; a standalone \
             runinator-background-worker must run it"
        );
    }
    if rate_limit.enabled {
        info!(
            requests_per_second = rate_limit.requests_per_second,
            burst = rate_limit.burst,
            "HTTP API rate limiting is ENABLED"
        );
    }
    if overload.enabled {
        info!(
            max_concurrent_requests = overload.max_concurrent_requests,
            request_timeout_seconds = overload.request_timeout.as_secs(),
            "HTTP API overload protection is ENABLED"
        );
    }
    let provisioner = Arc::new(runinator_provisioner::build_registry(
        crate::provisioner_config::from_env(),
    ));
    if !provisioner.is_empty() {
        info!("on-demand node provisioning is ENABLED");
    }
    let app = build_router(
        pool,
        bus,
        broker,
        provisioner,
        auth_config,
        rate_limit,
        overload,
    );
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr).await?;
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    );
    info!("Webserver started at {}:{}", addr.ip(), addr.port());

    tokio::select! {
        // graceful shutdown is checked first so normal teardown is never misreported as a loop
        // failure when a background task also winds down on the same notification.
        biased;
        _ = notify.notified() => {
            info!("Shutting down web server...");
            background.shutdown().await;
            Ok(())
        }
        result = server => {
            background.shutdown().await;
            if let Err(err) = result {
                error!("webserver error: {}", err);
                return Err(Box::new(err));
            }
            Ok(())
        }
        Some(joined) = background.join_next() => {
            // a long-lived loop only ends on its own via a panic or an early return, both of which
            // leave the orchestrator degraded. fail the whole replica so it restarts and resumes from
            // durable state rather than running on with a silently dead loop.
            match &joined {
                Err(err) if err.is_panic() => {
                    error!("background orchestration loop panicked; shutting down replica: {err}");
                }
                Err(err) => {
                    error!("background orchestration loop aborted; shutting down replica: {err}");
                }
                Ok(()) => {
                    error!("background orchestration loop exited unexpectedly; shutting down replica");
                }
            }
            crate::stability::record_background_loop_failure();
            notify.notify_waiters();
            background.shutdown().await;
            Err(runinator_engine::errors::BACKGROUND_LOOP_EXITED.bare())
        }
    }
}
