use runinator_broker::{
    BrokerClientConfig, BrokerConnectionMode, BrokerConsumerProfile, select_broker_connection,
};
use runinator_models::errors::SendableError;
use runinator_platform::startup::ProcessResources;
use tracing::{error, info};
use uuid::Uuid;

use runinator_waker::{config::parse_config, waker_loop};

mod service;
use service::WakerService;

#[tokio::main]
async fn main() -> Result<(), SendableError> {
    WakerService::new().run().await
}

async fn run_process() -> Result<(), SendableError> {
    info!("parsing waker config");
    let config = parse_config()?;
    info!(
        broker_mode = %config.broker_mode,
        broker_client_id = %config.broker_client_id,
        consumer_group = %config.waker_consumer_group,
        "waker starting as a broker wake consumer"
    );

    let process = ProcessResources::start("Runinator Waker")?;
    let broker_mode = BrokerConnectionMode::parse(&config.broker_mode)
        .ok_or_else(|| format!("unknown --broker-mode '{}'", config.broker_mode))?;
    let connection = select_broker_connection(
        broker_mode,
        BrokerClientConfig {
            backend: config.broker_backend.clone(),
            endpoint: config.broker_endpoint.clone(),
            effect_topic: config.broker_effect_topic.clone(),
            infrastructure_effect_topic: config.broker_infrastructure_effect_topic.clone(),
            control_topic: config.broker_control_topic.clone(),
            agent_topic: None,
            effect_result_topic: config.broker_effect_result_topic.clone(),
            client_id: config.broker_client_id.clone(),
            relay_credential: config.api_key.clone(),
            wake_topic: Some(config.broker_wake_topic.clone()),
            ingress_topic: Some(config.broker_ingress_topic.clone()),
        },
        config.service_url.clone().unwrap_or_default(),
        Some(&config.broker_relay_path),
    );
    let broker_connection = connection.description()?;
    let broker_backend = connection.client_config()?.backend;
    let broker = connection.connect(BrokerConsumerProfile::Waker).await?;
    let shutdown = process.shutdown();
    let notify = shutdown.notifier();
    let replica_id = Uuid::now_v7();
    let runtime_id = replica_id.to_string();
    let attributes = runinator_observability::resource_telemetry::attributes_with_host_metadata(
        &runinator_models::json!({
            "broker_backend": broker_backend,
            "broker_connection": broker_connection,
            "broker_client_id": config.broker_client_id.clone(),
            "consumer_group": config.waker_consumer_group.clone(),
        }),
    );
    runinator_waker::publish_replica_availability(
        broker.as_ref(),
        &config,
        replica_id,
        &runtime_id,
        attributes.clone(),
    )
    .await?;

    runinator_waker::spawn_liveness(&config, notify.clone());
    let heartbeat =
        runinator_waker::spawn_broker_heartbeat(broker.clone(), &config, notify.clone());
    let replica_heartbeat = runinator_waker::spawn_replica_heartbeat(
        broker.clone(),
        config.clone(),
        replica_id,
        runtime_id,
        attributes,
        notify.clone(),
    );

    let loop_notify = notify.clone();
    let loop_broker = broker.clone();
    let loop_config = config.clone();
    let handle = tokio::spawn(async move {
        waker_loop(loop_broker, loop_notify, &loop_config).await;
    });

    shutdown.cancelled().await;
    info!("received shutdown signal, shutting down");
    notify.notify_waiters();
    if let Err(err) = handle.await {
        error!("error while shutting down waker: {:?}", err);
    }
    if let Err(err) = heartbeat.await {
        error!("error while shutting down waker heartbeat: {:?}", err);
    }
    if let Err(err) = replica_heartbeat.await {
        error!(
            "error while shutting down waker replica heartbeat: {:?}",
            err
        );
    }

    info!("waker shutdown complete");
    Ok(())
}
