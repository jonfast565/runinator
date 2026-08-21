use std::sync::Arc;
use std::time::Duration;

use runinator_api::{AsyncApiClient, ReplicaClient, ReplicaServiceConfig, StaticLocator};
use runinator_models::errors::SendableError;
use runinator_models::replicas::ReplicaKind;
use runinator_service_bootstrap::{
    ApiClientConfig, BrokerClientConfig, BrokerConsumerProfile, ServerResources,
};
use runinator_utilities::resource_telemetry::{TelemetryCollector, attributes_with_host_metadata};
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
    info!(waker_id = %config.waker_id, "waker starting");

    let resources = ServerResources::builder("Runinator Waker")
        .broker(
            BrokerClientConfig {
                backend: config.broker_backend.clone(),
                endpoint: config.broker_endpoint.clone(),
                action_topic: config.broker_action_topic.clone(),
                control_topic: config.broker_control_topic.clone(),
                agent_topic: None,
                result_topic: config.broker_result_topic.clone(),
                client_id: config.broker_client_id.clone(),
                relay_credential: None,
                wake_topic: Some(config.broker_wake_topic.clone()),
                ingress_topic: Some(config.broker_ingress_topic.clone()),
            },
            BrokerConsumerProfile::Waker,
        )
        .api(ApiClientConfig {
            base_url: config.api_base_url.clone(),
            api_key: config.api_key.clone(),
        })
        .build()
        .await?;
    let shutdown = resources.process().shutdown();
    let notify = shutdown.notifier();
    let broker = resources.broker().expect("waker requested broker").clone();
    let api_client = resources.api().expect("waker requested api client");
    let service_config = ReplicaServiceConfig {
        replica_type: ReplicaKind::Waker,
        instance_id: config.waker_id.clone(),
        display_name: Some(config.waker_id.clone()),
        host: advertise_host(&config.advertise_host),
        port: None,
        base_path: None,
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
        attributes: attributes_with_host_metadata(&runinator_models::json!({
            "broker_backend": config.broker_backend,
            "broker_client_id": config.broker_client_id,
            "consumer_group": config.waker_consumer_group,
        })),
        heartbeat_interval: Duration::from_secs(10),
    };
    // registration is required: a waker that never registers is invisible in the replica registry
    // and cannot heartbeat, so retry with backoff and fail loudly rather than run as a phantom. stay
    // interruptible so ctrl_c during a retry window still shuts the process down cleanly.
    let replica_client = tokio::select! {
        result = register_waker_replica_with_retry(&api_client, &service_config) => result?,
        _ = shutdown.cancelled() => {
            info!("shutdown signal received before waker registration completed, shutting down");
            return Ok(());
        }
    };
    let _heartbeat = replica_client
        .spawn_heartbeat_with_telemetry(notify.clone(), Some(Arc::new(TelemetryCollector::new())));

    runinator_waker::spawn_liveness(&config, notify.clone());

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

    info!("waker shutdown complete");
    Ok(())
}

// registration retry envelope: waker startup keeps trying while the web service is briefly
// unreachable, then gives up so the process exits non-zero and the orchestrator restarts it.
const REGISTER_MAX_ATTEMPTS: u32 = 8;
const REGISTER_BASE_BACKOFF: Duration = Duration::from_secs(2);
const REGISTER_MAX_BACKOFF: Duration = Duration::from_secs(30);

// exponential backoff for the nth registration attempt (1-based), capped at REGISTER_MAX_BACKOFF.
fn register_backoff(attempt: u32) -> Duration {
    let factor = 1u32
        .checked_shl(attempt.saturating_sub(1))
        .unwrap_or(u32::MAX);
    REGISTER_BASE_BACKOFF
        .saturating_mul(factor)
        .min(REGISTER_MAX_BACKOFF)
}

// register with bounded retries and loud logging, returning an error once attempts are exhausted so
// the waker fails visibly instead of running unregistered.
async fn register_waker_replica_with_retry(
    api_client: &AsyncApiClient<StaticLocator>,
    service_config: &ReplicaServiceConfig,
) -> Result<ReplicaClient<StaticLocator>, SendableError> {
    let mut attempt = 1;
    loop {
        match ReplicaClient::register(api_client.clone(), service_config.clone()).await {
            Ok(session) => {
                if attempt > 1 {
                    info!(attempt, "waker replica registered");
                }
                return Ok(session);
            }
            Err(err) if attempt >= REGISTER_MAX_ATTEMPTS => {
                error!(
                    attempt,
                    error_code = runinator_models::errors::error_code_or_unknown(&err),
                    "failed to register waker replica, giving up: {}",
                    err
                );
                return Err(runinator_waker::errors::REPLICA_REGISTER.error(err));
            }
            Err(err) => {
                let backoff = register_backoff(attempt);
                error!(
                    attempt,
                    max_attempts = REGISTER_MAX_ATTEMPTS,
                    retry_in_secs = backoff.as_secs(),
                    error_code = runinator_models::errors::error_code_or_unknown(&err),
                    "failed to register waker replica, retrying: {}",
                    err
                );
                tokio::time::sleep(backoff).await;
                attempt += 1;
            }
        }
    }
}

// treat a blank advertise host as unset so the replica list omits it rather than storing "".
fn advertise_host(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}
