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
use runinator_broker::{Broker, IngressMessage};
use runinator_comm::WsIngressCommand;
use runinator_database::interfaces::DatabaseImpl;
use runinator_engine::{EngineConfig, EventSender, run_background_engine};
use runinator_models::errors::SendableError;
use runinator_models::replicas::{ReplicaKind, ReplicaRegistrationRequest};
use runinator_models::value::Value;
use runinator_service_bootstrap::{
    BrokerClientConfig, BrokerConsumerProfile, DatabaseRequest, ServerResources,
    dispatch_server_database,
};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::config::CliArgs;
use runinator_observability::resource_telemetry;
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

/// Run the durable engine while advertising this background runtime through broker ingress.
/// Web services write their own records directly; every other runtime, including this one, uses
/// the same broker lifecycle contract so availability has one durable ingestion path.
async fn run_engine_with_replica<T: DatabaseImpl>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    instance: String,
    attributes: Value,
    max_concurrent_ingress: usize,
    shutdown: Arc<Notify>,
) -> Result<(), SendableError> {
    let replica_id = Uuid::now_v7();
    let runtime_id = replica_id.to_string();
    let base_attributes = resource_telemetry::attributes_with_host_metadata(&attributes);
    publish_replica_availability(
        broker.as_ref(),
        replica_id,
        &runtime_id,
        &instance,
        base_attributes.clone(),
    )
    .await?;

    // Heartbeats and clean shutdown take the same ingress route as the initial announcement.
    let hb_shutdown = shutdown.clone();
    let hb_broker = broker.clone();
    let hb_replica_id = replica_id;
    let hb_runtime_id = runtime_id.clone();
    let hb_instance = instance.clone();
    let hb_attributes = base_attributes;
    let telemetry = Arc::new(resource_telemetry::TelemetryCollector::new());
    let heartbeat = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                _ = hb_shutdown.notified() => {
                    let command = WsIngressCommand::replica_offline(hb_replica_id, hb_runtime_id.clone());
                    let _ = hb_broker.publish_ingress(IngressMessage {
                        dedupe_key: Some(command.dedupe_key()),
                        command,
                        enqueued_at: chrono::Utc::now(),
                    }).await;
                    return;
                }
                _ = ticker.tick() => {
                    let attributes = resource_telemetry::attributes_with_telemetry(
                        &hb_attributes,
                        telemetry.as_ref(),
                    );
                    let _ = publish_replica_availability(
                        hb_broker.as_ref(),
                        hb_replica_id,
                        &hb_runtime_id,
                        &hb_instance,
                        attributes,
                    ).await;
                }
            }
        }
    });

    let publisher = EventSender::new(broker.clone());
    let result = run_background_engine(
        db,
        broker.clone(),
        publisher,
        None,
        instance,
        EngineConfig {
            max_concurrent_ingress,
        },
        shutdown,
    )
    .await;
    // Do not rely only on the heartbeat task observing shutdown: the engine can also return after
    // an internal error, and aborting that task first would leave this replica live until stale
    // reaping. The offline observation is idempotent with the heartbeat's shutdown branch.
    publish_replica_offline(broker.as_ref(), replica_id, &runtime_id).await;
    heartbeat.abort();
    result
}

async fn publish_replica_availability(
    broker: &dyn Broker,
    replica_id: Uuid,
    runtime_id: &str,
    instance: &str,
    attributes: Value,
) -> Result<(), runinator_broker::BrokerError> {
    let command = WsIngressCommand::replica_available(
        ReplicaRegistrationRequest {
            replica_id: Some(replica_id),
            replica_type: ReplicaKind::Background,
            instance_id: instance.to_string(),
            runtime_id: runtime_id.to_string(),
            display_name: Some(instance.to_string()),
            host: None,
            port: None,
            base_path: None,
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            attributes,
        },
        Vec::new(),
    );
    broker
        .publish_ingress(IngressMessage {
            dedupe_key: Some(command.dedupe_key()),
            command,
            enqueued_at: chrono::Utc::now(),
        })
        .await
}

async fn publish_replica_offline(broker: &dyn Broker, replica_id: Uuid, runtime_id: &str) {
    let command = WsIngressCommand::replica_offline(replica_id, runtime_id);
    if let Err(err) = broker
        .publish_ingress(IngressMessage {
            dedupe_key: Some(command.dedupe_key()),
            command,
            enqueued_at: chrono::Utc::now(),
        })
        .await
    {
        log::warn!("failed to announce background engine shutdown: {err}");
    }
}
