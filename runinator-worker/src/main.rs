use std::{env, ffi::OsString, sync::Arc, time::Duration};

use log::{error, info, warn};
use runinator_api::{
    AsyncApiClient, ReplicaServiceConfig, ReplicaSession, StaticLocator, register_replica_session,
    spawn_replica_heartbeat,
};
use runinator_comm::ConsumerProfile;
use runinator_models::errors::SendableError;
use runinator_models::replicas::ReplicaKind;
use runinator_utilities::startup;
use tokio::sync::Notify;

use runinator_worker::{
    Config, WorkerRuntime, build_broker, default_provider_factory, errors, load_libraries,
    parse_config, start_worker_loop,
};

#[cfg(test)]
mod main_tests;

// touches the configured liveness file on an interval until shutdown; used by the k8s exec probe.
fn spawn_liveness(config: &Config, shutdown: Arc<Notify>) -> Option<tokio::task::JoinHandle<()>> {
    runinator_utilities::liveness::spawn_liveness(
        &config.liveness_file,
        runinator_utilities::liveness::DEFAULT_LIVENESS_INTERVAL,
        shutdown,
    )
}

fn main() -> Result<(), SendableError> {
    startup::startup("Runinator Worker")?;

    let config = parse_config()?;
    configure_provider_service_url(&config);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| errors::RUNTIME_BUILD.error(err))?;
    runtime.block_on(run(config))
}

async fn run(config: Config) -> Result<(), SendableError> {
    info!("Worker ID: {}", config.worker_id);

    let libraries = Arc::new(load_libraries(&config.dll_paths)?);
    let broker = build_broker(&config).await?;
    let api_client = build_api_client(&config)?;
    let shutdown = Arc::new(Notify::new());

    spawn_liveness(&config, shutdown.clone());
    let replica_session = match register_worker_replica(&api_client, &config).await {
        Ok(session) => Some(session),
        Err(err) => {
            warn!("Failed to register worker replica: {}", err);
            None
        }
    };
    let _heartbeat = replica_session
        .clone()
        .map(|session| spawn_replica_heartbeat(api_client.clone(), session, shutdown.clone()));
    let mut worker_task = {
        let runtime = WorkerRuntime {
            broker: broker.clone(),
            profile: ConsumerProfile::shared(config.broker_consumer_id.clone()),
            libraries: Arc::clone(&libraries),
            api_client: api_client.clone(),
            replica_id: replica_session.as_ref().map(ReplicaSession::replica_id),
            providers: default_provider_factory(),
            max_concurrent_actions: config.max_concurrent_actions,
            shutdown_grace: Duration::from_secs(config.shutdown_grace_seconds),
            shutdown: shutdown.clone(),
        };
        tokio::spawn(start_worker_loop(runtime))
    };

    tokio::select! {
        signal = tokio::signal::ctrl_c() => {
            signal.map_err(|err| errors::SIGNAL_CTRL_C.error(err))?;
            info!("Shutdown signal received. Stopping worker...");
            shutdown.notify_waiters();
        }
        result = &mut worker_task => {
            return handle_worker_task_result(result);
        }
    }

    match tokio::time::timeout(
        Duration::from_secs(config.shutdown_grace_seconds + 5),
        &mut worker_task,
    )
    .await
    {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(err))) => return Err(err),
        Ok(Err(err)) if err.is_cancelled() => {}
        Ok(Err(err)) => {
            error!("Worker task join error: {}", err);
        }
        Err(_) => {
            error!("Worker shutdown grace period elapsed before the loop stopped");
            worker_task.abort();
        }
    }

    Ok(())
}

fn configure_provider_service_url(config: &Config) {
    let Some(value) =
        provider_service_url_fallback(env::var_os("RUNINATOR_SERVICE_URL"), &config.api_base_url)
    else {
        return;
    };

    // safety: this runs before the worker starts provider execution or spawns runtime work.
    unsafe {
        env::set_var("RUNINATOR_SERVICE_URL", value);
    }
}

fn provider_service_url_fallback(
    existing: Option<OsString>,
    api_base_url: &str,
) -> Option<OsString> {
    if existing
        .as_ref()
        .is_some_and(|value| !value.to_string_lossy().trim().is_empty())
    {
        return None;
    }
    Some(OsString::from(api_base_url))
}

fn handle_worker_task_result(
    result: Result<Result<(), SendableError>, tokio::task::JoinError>,
) -> Result<(), SendableError> {
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => {
            error!("Worker loop terminated with error: {}", err);
            Err(err)
        }
        Err(err) => {
            error!("Worker task join error: {}", err);
            Err(errors::LOOP_JOIN.error(err))
        }
    }
}

fn build_api_client(config: &Config) -> Result<AsyncApiClient<StaticLocator>, SendableError> {
    let locator = StaticLocator::new(config.api_base_url.clone());
    AsyncApiClient::with_credentials(locator, config.api_key.clone())
        .map_err(|err| errors::API_CLIENT.error(err))
}

async fn register_worker_replica(
    api_client: &AsyncApiClient<StaticLocator>,
    config: &Config,
) -> Result<ReplicaSession, runinator_api::ApiError> {
    register_replica_session(
        api_client,
        ReplicaServiceConfig {
            replica_type: ReplicaKind::Worker,
            instance_id: config.worker_id.to_string(),
            display_name: Some(format!("worker-{}", config.worker_id)),
            host: config.advertise_host.clone(),
            port: None,
            base_path: None,
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            attributes: runinator_models::json!({
                "broker_backend": config.broker_backend,
                "broker_client_id": config.broker_client_id,
                "broker_consumer_id": config.broker_consumer_id,
            }),
            heartbeat_interval: Duration::from_secs(10),
        },
    )
    .await
}
