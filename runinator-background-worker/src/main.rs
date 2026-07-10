//! standalone durable orchestration engine.
//!
//! runs the same `runinator_engine::run_background_engine` the web service can embed in-process,
//! but as a separately deployable, horizontally-scalable process: it opens its own database pool and
//! broker connection, registers as a `background` replica, and drives the reducer, wake/trigger/
//! action/ingress loops, result consumer, and maintenance backstops. deploy it alongside
//! `runinator-ws` started with `RUNINATOR_WS_RUN_ENGINE=false` so HTTP and engine tiers scale
//! independently; multiple instances run active/active via the engine's durable claim/lease
//! coordination.

mod config;

use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use log::info;
use runinator_broker::{
    Broker,
    adapters::{kafka::KafkaBrokerConfig, rabbitmq::RabbitMqBrokerConfig},
    http::client::HttpBroker,
    in_memory::InMemoryBroker,
    tcp::client::TcpBroker,
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_db_cli::{DatabaseBackend, dispatch_database};
use runinator_engine::{EnginePublisher, run_background_engine};
use runinator_models::auth::AuthContext;
use runinator_models::errors::SendableError;
use runinator_models::replicas::{
    ReplicaHeartbeatRequest, ReplicaKind, ReplicaRegistrationRequest,
};
use runinator_models::value::Value;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::config::CliArgs;
use runinator_utilities::{app_data, resource_telemetry, startup};

#[tokio::main]
async fn main() -> Result<(), SendableError> {
    // the broker's http/tcp transports and the aws sdk both link rustls; install a process-default
    // CryptoProvider before any rustls default-path config is built. an Err means one is already
    // installed, which is fine.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // held for the process lifetime so otel signals flush on shutdown.
    let _telemetry = startup::startup("Runinator Background Worker")?;

    let args = CliArgs::parse();

    let notify = Arc::new(Notify::new());
    let shutdown_listener = notify.clone();
    tokio::spawn(async move {
        if let Err(err) = tokio::signal::ctrl_c().await {
            log::error!("Failed to listen for shutdown signal: {}", err);
            return;
        }
        info!("Shutdown signal received, stopping background worker...");
        shutdown_listener.notify_waiters();
    });

    let CliArgs {
        database,
        sqlite_path,
        database_url,
        broker_backend,
        broker_endpoint,
        broker_action_topic,
        broker_control_topic,
        broker_result_topic,
        broker_client_id,
        instance_id,
    } = args;

    // a stable per-process id used when claiming trigger/action-dispatch rows; k8s passes the pod name.
    let instance = instance_id
        .and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .unwrap_or_else(|| format!("runinator-background-worker-{}", Uuid::new_v4()));

    // kept for the advertised attributes since the broker configs consume the original below.
    let broker_client_id_display = broker_client_id.clone();

    let broker = build_broker(
        &broker_backend,
        &broker_endpoint,
        KafkaBrokerConfig::new(broker_endpoint.clone())
            .with_topics(
                broker_action_topic.clone(),
                broker_control_topic.clone(),
                broker_result_topic.clone(),
            )
            .with_client_id(broker_client_id.clone()),
        RabbitMqBrokerConfig::new(broker_endpoint.clone())
            .with_queues(
                broker_action_topic,
                broker_control_topic,
                broker_result_topic,
            )
            .with_client_id(broker_client_id),
    )
    .await?;

    let database_backend = match &database {
        DatabaseBackend::Sqlite => "sqlite",
        DatabaseBackend::Postgres => "postgres",
        DatabaseBackend::Mysql => "mysql",
    };
    // advertised so this worker's replica record has backend parity with ws/worker/waker.
    let attributes = runinator_models::json!({
        "broker_backend": broker_backend,
        "broker_client_id": broker_client_id_display,
        "database_backend": database_backend,
    });
    info!("Starting Runinator background worker with {database_backend} database as {instance}");
    dispatch_database!(
        database,
        sqlite: {
            let sqlite_path = sqlite_path.unwrap_or(app_data::default_sqlite_path()?);
            if let Some(parent) = sqlite_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            info!("SQLite database file at {}", sqlite_path.display());
            sqlite_path.to_string_lossy().into_owned()
        },
        url: database_url
            .clone()
            .ok_or_else(|| -> SendableError {
                "--database-url must be provided when --database=postgres/mysql/mariadb".into()
            })?,
        |db| {
            run_engine_with_replica(
                db,
                broker.clone(),
                instance.clone(),
                attributes.clone(),
                notify.clone(),
            )
            .await?;
        }
    );

    Ok(())
}

/// register this process as a `Background` replica, run a heartbeat alongside the engine so it stays
/// live in the fleet view, drive the durable engine, and mark the replica offline on shutdown.
async fn run_engine_with_replica<T: DatabaseImpl>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    instance: String,
    attributes: Value,
    shutdown: Arc<Notify>,
) -> Result<(), SendableError> {
    let runtime_id = Uuid::new_v4().to_string();
    let replica = runinator_engine::repository::register_replica(
        db.as_ref(),
        ReplicaRegistrationRequest {
            replica_type: ReplicaKind::Background,
            instance_id: instance.clone(),
            runtime_id: runtime_id.clone(),
            display_name: Some(instance.clone()),
            host: None,
            port: None,
            base_path: None,
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            attributes: resource_telemetry::attributes_with_host_metadata(&attributes),
        },
        None,
        // the worker registering its own replica at startup, not an external caller.
        &AuthContext::disabled_admin(),
    )
    .await?;

    // heartbeat loop: keeps the replica live and appends resource telemetry each tick, and marks the
    // replica offline on shutdown. best-effort, so a failed heartbeat never tears down the process.
    let hb_db = db.clone();
    let hb_shutdown = shutdown.clone();
    let hb_replica_id = replica.replica_id;
    let hb_runtime_id = runtime_id.clone();
    let hb_instance = instance.clone();
    let hb_attributes = attributes.clone();
    let telemetry = Arc::new(resource_telemetry::TelemetryCollector::new());
    let heartbeat = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                _ = hb_shutdown.notified() => {
                    let _ = runinator_engine::repository::mark_replica_offline(
                        hb_db.as_ref(),
                        hb_replica_id,
                        hb_runtime_id.clone(),
                    )
                    .await;
                    return;
                }
                _ = ticker.tick() => {
                    let attributes = resource_telemetry::attributes_with_telemetry(
                        &hb_attributes,
                        telemetry.as_ref(),
                    );
                    let _ = runinator_engine::repository::heartbeat_replica(
                        hb_db.as_ref(),
                        hb_replica_id,
                        ReplicaHeartbeatRequest {
                            runtime_id: hb_runtime_id.clone(),
                            display_name: Some(hb_instance.clone()),
                            host: None,
                            port: None,
                            base_path: None,
                            attributes,
                        },
                        None,
                    )
                    .await;
                }
            }
        }
    });

    let publisher = EnginePublisher::new(broker.clone());
    let result = run_background_engine(db, broker, publisher, instance, shutdown).await;
    heartbeat.abort();
    result
}

async fn build_broker(
    backend: &str,
    endpoint: &str,
    kafka_config: KafkaBrokerConfig,
    rabbitmq_config: RabbitMqBrokerConfig,
) -> Result<Arc<dyn Broker>, SendableError> {
    let result_channel = match backend {
        "kafka" => kafka_config.result_topic.as_str(),
        "rabbitmq" => rabbitmq_config.result_queue.as_str(),
        _ => "",
    };
    runinator_broker::ensure_named_workflow_result_channel(backend, result_channel)
        .map_err(|err| -> SendableError { err.into() })?;

    let broker: Arc<dyn Broker> = match backend {
        "http" => {
            let url = reqwest::Url::parse(endpoint).map_err(|err| -> SendableError {
                format!("invalid broker endpoint '{endpoint}': {err}").into()
            })?;
            let client = reqwest::Client::builder()
                .build()
                .map_err(|err| -> SendableError { err.to_string().into() })?;
            Arc::new(HttpBroker::new(url, client))
        }
        "in-memory" => Arc::new(InMemoryBroker::new()),
        "tcp" => Arc::new(TcpBroker::new(endpoint.to_string())),
        "kafka" => runinator_broker::build_kafka_broker(kafka_config)
            .map_err(|err| -> SendableError { err.into() })?,
        "rabbitmq" => runinator_broker::build_rabbitmq_broker(rabbitmq_config)
            .await
            .map_err(|err| -> SendableError { err.into() })?,
        other => {
            return Err(format!("unknown broker backend '{other}'").into());
        }
    };

    runinator_broker::ensure_workflow_result_channels_supported(backend, broker.as_ref())
        .map_err(|err| -> SendableError { err.into() })?;

    // wrap the concrete backend so every broker operation emits otel metrics tagged with the backend.
    Ok(runinator_broker::instrument(broker, backend.to_string()))
}
