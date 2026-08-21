//! standalone durable orchestration engine worker.
//!
//! runs the same `runinator_engine::run_background_engine` the web service can embed in-process,
//! but as a separately deployable, horizontally-scalable process: it opens its own database pool and
//! broker connection, registers as a `background` replica, and drives the workflow VM/effect
//! loops, trigger and agent-directive publishers, and maintenance backstops. deploy it alongside
//! `runinator-ws` started with `RUNINATOR_WS_RUN_ENGINE=false` so HTTP and engine tiers scale
//! independently; multiple instances run active/active via the engine's durable claim/lease
//! coordination.

mod config;
mod service;

use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use log::info;
use runinator_broker::Broker;
use runinator_database::interfaces::DatabaseImpl;
use runinator_engine::{EngineConfig, EnginePublisher, run_background_engine};
use runinator_models::auth::AuthContext;
use runinator_models::errors::SendableError;
use runinator_models::replicas::{
    ReplicaHeartbeatRequest, ReplicaKind, ReplicaRegistrationRequest,
};
use runinator_models::value::Value;
use runinator_service_bootstrap::{
    BrokerClientConfig, BrokerConsumerProfile, DatabaseRequest, ServerResources,
    dispatch_server_database,
};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::config::CliArgs;
use runinator_utilities::resource_telemetry;
use service::EngineWorkerService;

#[tokio::main]
async fn main() -> Result<(), SendableError> {
    EngineWorkerService::new().run().await
}

async fn run_process() -> Result<(), SendableError> {
    // the broker's http/tcp transports and the aws sdk both link rustls; install a process-default
    // crypto provider before any rustls default-path config is built. an err means one is already
    // installed, which is fine.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let args = CliArgs::parse();

    let CliArgs {
        database,
        sqlite_path,
        database_url,
        broker_backend,
        broker_endpoint,
        broker_action_topic,
        broker_control_topic,
        broker_agent_topic,
        broker_result_topic,
        broker_client_id,
        instance_id,
        max_concurrent_ingress,
    } = args;

    // a stable per-process id used when claiming trigger/action-dispatch rows; k8s passes the pod name.
    let instance = instance_id
        .and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .unwrap_or_else(|| format!("runinator-engine-worker-{}", Uuid::new_v4()));

    // kept for the advertised attributes since the broker configs consume the original below.
    let broker_client_id_display = broker_client_id.clone();

    let resources = ServerResources::builder("Runinator Engine Worker")
        .broker(
            BrokerClientConfig {
                backend: broker_backend.clone(),
                endpoint: broker_endpoint,
                action_topic: broker_action_topic,
                control_topic: broker_control_topic,
                agent_topic: Some(broker_agent_topic),
                result_topic: broker_result_topic,
                client_id: broker_client_id,
                relay_credential: None,
                wake_topic: None,
                ingress_topic: None,
            },
            BrokerConsumerProfile::WorkflowRuntime,
        )
        .database(DatabaseRequest {
            backend: database,
            sqlite_path,
            database_url,
        })
        .build()
        .await?;
    let notify = resources.process().shutdown().notifier();
    let broker = resources
        .broker()
        .expect("engine worker requested broker")
        .clone();
    let database = resources
        .database()
        .expect("engine worker requested database");

    let database_backend = database.backend().label();
    // advertised so this worker's replica record has backend parity with ws/worker/waker.
    let attributes = runinator_models::json!({
        "broker_backend": broker_backend,
        "broker_client_id": broker_client_id_display,
        "database_backend": database_backend,
    });
    info!("Starting Runinator engine worker with {database_backend} database as {instance}");
    dispatch_server_database!(database, |db| {
        run_engine_with_replica(
            db,
            broker.clone(),
            instance.clone(),
            attributes.clone(),
            max_concurrent_ingress,
            notify.clone(),
        )
        .await?;
    });

    Ok(())
}

/// register this process as a `Background` replica, run a heartbeat alongside the engine so it stays
/// live in the fleet view, drive the durable engine, and mark the replica offline on shutdown.
async fn run_engine_with_replica<T: DatabaseImpl>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    instance: String,
    attributes: Value,
    max_concurrent_ingress: usize,
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
        &AuthContext::disabled_platform_admin(),
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
    let result = run_background_engine(
        db,
        broker,
        publisher,
        instance,
        EngineConfig {
            max_concurrent_ingress,
        },
        shutdown,
    )
    .await;
    heartbeat.abort();
    result
}
