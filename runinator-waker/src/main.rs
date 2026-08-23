use runinator_broker::{BrokerConsumerProfile, build_broker_client};
use runinator_models::errors::SendableError;
use runinator_platform::startup::ProcessResources;
use tracing::{error, info};

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
        broker_backend = %config.broker_backend,
        broker_client_id = %config.broker_client_id,
        consumer_group = %config.waker_consumer_group,
        "waker starting as a broker wake consumer"
    );

    let process = ProcessResources::start("Runinator Waker")?;
    let broker = build_broker_client(
        &runinator_broker::BrokerClientConfig {
            backend: config.broker_backend.clone(),
            endpoint: config.broker_endpoint.clone(),
            effect_topic: config.broker_effect_topic.clone(),
            infrastructure_effect_topic: config.broker_infrastructure_effect_topic.clone(),
            control_topic: config.broker_control_topic.clone(),
            agent_topic: None,
            effect_result_topic: config.broker_effect_result_topic.clone(),
            client_id: config.broker_client_id.clone(),
            relay_credential: None,
            wake_topic: Some(config.broker_wake_topic.clone()),
            ingress_topic: Some(config.broker_ingress_topic.clone()),
        },
        BrokerConsumerProfile::Waker,
    )
    .await?;
    let shutdown = process.shutdown();
    let notify = shutdown.notifier();

    runinator_waker::spawn_liveness(&config, notify.clone());
    let heartbeat =
        runinator_waker::spawn_broker_heartbeat(broker.clone(), &config, notify.clone());

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

    info!("waker shutdown complete");
    Ok(())
}
