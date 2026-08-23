//! standalone durable orchestration engine worker.
//!
//! Runs the same `runinator_engine::run_background_engine` that the web service can embed.
//! This process has its own database pool and broker connection, registers as a `background`
//! replica, and runs the workflow, trigger, directive, and maintenance loops.
//! Deploy it beside `runinator-ws` with `RUNINATOR_WS_RUN_ENGINE=false` to scale HTTP and engine
//! workers independently. Durable claims and leases allow multiple instances to run together.

mod config;
mod service;

use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use log::info;
use runinator_broker::Broker;
use runinator_database::interfaces::DatabaseImpl;
use runinator_engine::{
    EngineConfig, EventSender, run_background_engine, services::ReplicaRegistry,
};
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
    // The broker's HTTP/TCP transports and the AWS SDK both link rustls. Install a process-default
    // crypto provider before building any rustls default configuration. An error means one is already
    // installed, which is fine.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let args = CliArgs::parse();

    let CliArgs {
        database,
        sqlite_path,
        database_url,
        broker_backend,
        broker_endpoint,
        broker_effect_topic,
        broker_infrastructure_effect_topic,
        broker_control_topic,
        broker_agent_topic,
        broker_effect_result_topic,
        broker_wake_topic,
        broker_ingress_topic,
        broker_client_id,
        instance_id,
        max_concurrent_ingress,
    } = args;

    // Use a stable per-process ID when claiming trigger/action-dispatch rows. Kubernetes passes the pod name.
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
                effect_topic: broker_effect_topic,
                infrastructure_effect_topic: broker_infrastructure_effect_topic,
                control_topic: broker_control_topic,
                agent_topic: Some(broker_agent_topic),
                effect_result_topic: broker_effect_result_topic,
                client_id: broker_client_id,
                relay_credential: None,
                // the engine arms timer wakes and consumes ingress, so it opts into both
                // orchestration channels rather than leaving them at the backend defaults.
                wake_topic: Some(broker_wake_topic),
                ingress_topic: Some(broker_ingress_topic),
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
    // Advertise it so this worker's replica record matches the WS, worker, and waker records.
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
    let registry = ReplicaRegistry::new(db.clone());
    let replica = registry
        .register(
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
    let hb_registry = registry.clone();
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
                    let _ = hb_registry
                        .mark_offline(hb_replica_id, hb_runtime_id.clone())
                        .await;
                    return;
                }
                _ = ticker.tick() => {
                    let attributes = resource_telemetry::attributes_with_telemetry(
                        &hb_attributes,
                        telemetry.as_ref(),
                    );
                    let _ = hb_registry.heartbeat(
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

    let publisher = EventSender::new(broker.clone());
    let result = run_background_engine(
        db,
        broker,
        publisher,
        None,
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
