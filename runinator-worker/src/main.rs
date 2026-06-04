mod broker;
mod config;
mod executor;
mod output_sink;
mod provider_repository;
mod secrets;

use std::{collections::HashMap, env, ffi::OsString, sync::Arc, time::Duration};

use config::parse_config;
use log::{error, info, warn};
use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_broker::{Broker, ControlDelivery};
use runinator_comm::ControlKind;
use runinator_comm::WireCodec;
use runinator_models::errors::{RuntimeError, SendableError};
use runinator_models::workflow_state::TaskStatusOutput;
use runinator_models::workflows::WorkflowStatus;
use runinator_plugin::{
    cancel::CancellationToken, load_libraries_from_path, plugin::Plugin, print_libs,
};
use runinator_utilities::startup;
use tokio::{
    sync::{Mutex, Notify, Semaphore},
    task::JoinSet,
};

use crate::output_sink::RunOutputSink;
use broker::{broker_error, build_broker};
use secrets::resolve_secret_refs;

#[cfg(test)]
mod tests;

fn main() -> Result<(), SendableError> {
    startup::startup("Runinator Worker")?;

    let config = parse_config()?;
    configure_provider_service_url(&config);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| {
            Box::new(RuntimeError::new("worker.runtime".into(), err.to_string())) as SendableError
        })?;
    runtime.block_on(run(config))
}

async fn run(config: config::Config) -> Result<(), SendableError> {
    info!("Worker ID: {}", config.worker_id);

    let libraries = Arc::new(load_libraries(&config.dll_paths)?);
    let broker = build_broker(&config).await?;
    let api_client = build_api_client(&config)?;
    publish_provider_metadata(&api_client).await;

    let shutdown = Arc::new(Notify::new());
    let mut worker_task = {
        let broker = broker.clone();
        let libraries = Arc::clone(&libraries);
        let api_client = api_client.clone();
        let consumer = config.broker_consumer_id.clone();
        let max_concurrent_actions = config.max_concurrent_actions;
        let shutdown_grace = Duration::from_secs(config.shutdown_grace_seconds);
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            run_worker_loop(
                broker,
                consumer,
                libraries,
                api_client,
                max_concurrent_actions,
                shutdown_grace,
                shutdown,
            )
            .await
        })
    };

    tokio::select! {
        signal = tokio::signal::ctrl_c() => {
            signal.map_err(|err| {
                Box::new(RuntimeError::new(
                    "worker.signal.ctrl_c".into(),
                    format!("Failed to listen for Ctrl+C: {err}"),
                )) as SendableError
            })?;
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

fn configure_provider_service_url(config: &config::Config) {
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
            Err(Box::new(RuntimeError::new(
                "worker.loop.join".into(),
                err.to_string(),
            )))
        }
    }
}

fn load_libraries(paths: &[String]) -> Result<HashMap<String, Plugin>, SendableError> {
    let mut libraries = HashMap::new();
    for path in paths {
        if !std::path::Path::new(path).exists() {
            info!("Skipping missing plugin path {}", path);
            continue;
        }

        info!("Loading plugins from {}", path);
        libraries.extend(load_libraries_from_path(path)?);
    }
    print_libs(&libraries);
    Ok(libraries)
}

fn build_api_client(
    config: &config::Config,
) -> Result<AsyncApiClient<StaticLocator>, SendableError> {
    let locator = StaticLocator::new(config.api_base_url.clone());
    AsyncApiClient::new(locator).map_err(|err| {
        Box::new(RuntimeError::new(
            "worker.api.client".into(),
            err.to_string(),
        )) as SendableError
    })
}

// register the built-in providers with the web service on startup. best-effort: a failure here
// is logged but does not stop the worker, which can still execute already-registered providers.
async fn publish_provider_metadata(api_client: &AsyncApiClient<StaticLocator>) {
    let bundle = provider_repository::metadata_bundle();
    let count = bundle.providers.len();
    match api_client.import_provider_bundle(&bundle).await {
        Ok(imported) => info!(
            "Registered {} provider(s) with the web service",
            imported.providers.len()
        ),
        Err(err) => warn!("Failed to register provider bundle ({count} provider(s)): {err}"),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_worker_loop(
    broker: Arc<dyn Broker>,
    consumer_id: String,
    libraries: Arc<HashMap<String, Plugin>>,
    api_client: AsyncApiClient<StaticLocator>,
    max_concurrent_actions: usize,
    shutdown_grace: Duration,
    shutdown: Arc<Notify>,
) -> Result<(), SendableError> {
    let max_concurrent_actions = max_concurrent_actions.max(1);
    let semaphore = Arc::new(Semaphore::new(max_concurrent_actions));
    let in_flight = Arc::new(Mutex::new(HashMap::<i64, CancellationToken>::new()));
    let control_task = tokio::spawn(run_control_loop(
        broker.clone(),
        consumer_id.clone(),
        Arc::clone(&in_flight),
        shutdown.clone(),
    ));
    let mut deliveries = JoinSet::new();
    info!("Worker processing up to {max_concurrent_actions} concurrent action(s)");

    loop {
        let permit = tokio::select! {
            biased;
            _ = shutdown.notified() => {
                info!("Worker loop shutting down");
                break;
            }
            Some(result) = deliveries.join_next(), if !deliveries.is_empty() => {
                if let Err(err) = result {
                    error!("Worker delivery task join error: {}", err);
                }
                continue;
            }
            permit = semaphore.clone().acquire_owned() => {
                permit.map_err(|err| {
                    Box::new(RuntimeError::new(
                        "worker.concurrency.closed".into(),
                        err.to_string(),
                    )) as SendableError
                })?
            }
        };

        let maybe_delivery = tokio::select! {
            _ = shutdown.notified() => {
                drop(permit);
                info!("Worker loop shutting down");
                break;
            }
            result = broker.receive(&consumer_id) => {
                result.map_err(|err| broker_error("receive", err))?
            }
        };

        let broker = broker.clone();
        let consumer_id = consumer_id.clone();
        let libraries = Arc::clone(&libraries);
        let api_client = api_client.clone();
        let in_flight = Arc::clone(&in_flight);
        deliveries.spawn(async move {
            let _permit = permit;
            if let Err(err) = process_delivery(
                &broker,
                &consumer_id,
                libraries,
                api_client,
                maybe_delivery,
                in_flight,
            )
            .await
            {
                error!("Error processing task: {}", err);
            }
        });
    }

    cancel_in_flight(&in_flight).await;
    match tokio::time::timeout(shutdown_grace, drain_deliveries(&mut deliveries)).await {
        Ok(()) => {}
        Err(_) => {
            warn!(
                "Worker shutdown grace period of {} second(s) elapsed; aborting unfinished action tasks",
                shutdown_grace.as_secs()
            );
            deliveries.abort_all();
            drain_deliveries(&mut deliveries).await;
        }
    }

    match control_task.await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => error!("Worker control loop terminated with error: {}", err),
        Err(err) if err.is_cancelled() => {}
        Err(err) => error!("Worker control task join error: {}", err),
    }

    Ok(())
}

async fn cancel_in_flight(in_flight: &Arc<Mutex<HashMap<i64, CancellationToken>>>) {
    let tokens = {
        let guard = in_flight.lock().await;
        guard.values().cloned().collect::<Vec<_>>()
    };
    if tokens.is_empty() {
        return;
    }
    warn!(
        "Canceling {} in-flight action(s) during worker shutdown",
        tokens.len()
    );
    for token in tokens {
        token.cancel();
    }
}

async fn drain_deliveries(deliveries: &mut JoinSet<()>) {
    while let Some(result) = deliveries.join_next().await {
        if let Err(err) = result {
            error!("Worker delivery task join error: {}", err);
        }
    }
}

async fn run_control_loop(
    broker: Arc<dyn Broker>,
    consumer_id: String,
    in_flight: Arc<Mutex<HashMap<i64, CancellationToken>>>,
    shutdown: Arc<Notify>,
) -> Result<(), SendableError> {
    loop {
        let delivery = tokio::select! {
            _ = shutdown.notified() => {
                info!("Worker control loop shutting down");
                return Ok(());
            }
            result = broker.receive_control(&consumer_id) => {
                result.map_err(|err| broker_error("receive_control", err))?
            }
        };
        handle_control_delivery(&broker, &consumer_id, &in_flight, delivery).await?;
    }
}

async fn handle_control_delivery(
    broker: &Arc<dyn Broker>,
    consumer_id: &str,
    in_flight: &Arc<Mutex<HashMap<i64, CancellationToken>>>,
    delivery: ControlDelivery,
) -> Result<(), SendableError> {
    let control_kind = delivery.command.kind;
    match control_kind {
        ControlKind::Cancel => {
            let token = {
                let guard = in_flight.lock().await;
                guard.get(&delivery.command.workflow_run_id).cloned()
            };
            if let Some(token) = token {
                token.cancel();
                info!(
                    "Cancellation requested for workflow run {}",
                    delivery.command.workflow_run_id
                );
            } else {
                info!(
                    "Cancellation requested for workflow run {}, but no local execution is active",
                    delivery.command.workflow_run_id
                );
            }
        }
        ControlKind::Pause => {
            info!(
                "Pause control received for workflow run {}; the web service will stop dispatching at the next boundary",
                delivery.command.workflow_run_id
            );
        }
        ControlKind::Resume => {
            info!(
                "Resume control received for workflow run {}; the web service controls dispatch resumption",
                delivery.command.workflow_run_id
            );
        }
    }
    broker
        .ack_control(consumer_id, delivery.delivery_id)
        .await
        .map_err(|err| broker_error("ack_control", err))
}

async fn process_delivery(
    broker: &Arc<dyn Broker>,
    consumer_id: &str,
    libraries: Arc<HashMap<String, Plugin>>,
    api_client: AsyncApiClient<StaticLocator>,
    delivery: runinator_broker::BrokerDelivery,
    in_flight: Arc<Mutex<HashMap<i64, CancellationToken>>>,
) -> Result<(), SendableError> {
    let command = delivery.command.clone();
    let action = command.action.clone();
    let token = CancellationToken::new();
    in_flight
        .lock()
        .await
        .insert(command.workflow_run_id, token.clone());
    let sink = RunOutputSink::new(
        command.clone(),
        broker.clone(),
        tokio::runtime::Handle::current(),
    );
    if let Err(err) = sink
        .publish_status(WorkflowStatus::Running, None, None)
        .await
    {
        error!(
            "Failed to publish workflow node run {} running status: {}",
            command.workflow_node_run_id, err
        );
        in_flight.lock().await.remove(&command.workflow_run_id);
        nack_action_delivery(broker, consumer_id, delivery.delivery_id).await?;
        return Err(broker_error("publish_result", err));
    }
    let parameters = match resolve_secret_refs(&api_client, command.parameters.clone()).await {
        Ok(parameters) => parameters,
        Err(err) => {
            let message = format!("Failed to resolve action secrets: {err}");
            error!("{}", message);
            let output_json = TaskStatusOutput {
                success: false,
                duration_ms: None,
                message: Some(message.clone()),
            }
            .to_wire_value()?;
            if let Err(err) = sink
                .publish_status(
                    WorkflowStatus::Failed,
                    Some(output_json),
                    Some(message.clone()),
                )
                .await
            {
                error!(
                    "Failed to publish workflow node run {} failed status: {}",
                    command.workflow_node_run_id, err
                );
                in_flight.lock().await.remove(&command.workflow_run_id);
                nack_action_delivery(broker, consumer_id, delivery.delivery_id).await?;
                return Err(broker_error("publish_result", err));
            }
            broker
                .ack(consumer_id, delivery.delivery_id)
                .await
                .map_err(|err| broker_error("ack", err))?;
            in_flight.lock().await.remove(&command.workflow_run_id);
            return Ok(());
        }
    };
    let result = executor::execute_task(
        libraries,
        action.clone(),
        command.workflow_node_run_id,
        parameters,
        Some(Arc::new(sink.clone())),
        token,
    )
    .await;
    in_flight.lock().await.remove(&command.workflow_run_id);
    if let Some(execution_result) = &result.execution_result
        && let Err(err) = sink.persist_result(execution_result).await
    {
        error!(
            "Failed to publish workflow node run {} result artifacts: {}",
            command.workflow_node_run_id, err
        );
        nack_action_delivery(broker, consumer_id, delivery.delivery_id).await?;
        return Err(broker_error("publish_result", err));
    }
    let task_result = result.task_result;
    let provider_message = task_result.message.clone().or_else(|| sink.message());

    if task_result.success {
        sink.emit_log(format!(
            "Action {}.{} completed successfully in {} ms.",
            action.provider,
            action.function,
            task_result.duration_ms()
        ));
        if let Err(err) = sink.flush().await {
            nack_action_delivery(broker, consumer_id, delivery.delivery_id).await?;
            return Err(broker_error("publish_result", err));
        }

        let output_json = result
            .execution_result
            .as_ref()
            .and_then(|execution_result| execution_result.output_json.clone())
            .map(Ok)
            .unwrap_or_else(|| {
                TaskStatusOutput {
                    success: true,
                    duration_ms: Some(task_result.duration_ms()),
                    message: provider_message.clone(),
                }
                .to_wire_value()
            })?;
        if let Err(err) = sink
            .publish_status(
                WorkflowStatus::Succeeded,
                Some(output_json),
                provider_message.clone(),
            )
            .await
        {
            error!(
                "Failed to publish workflow node run {} succeeded status: {}",
                command.workflow_node_run_id, err
            );
            nack_action_delivery(broker, consumer_id, delivery.delivery_id).await?;
            return Err(broker_error("publish_result", err));
        }
    } else {
        sink.emit_log(format!(
            "Action {}.{} failed after {} ms: {}.",
            action.provider,
            action.function,
            task_result.duration_ms(),
            provider_message.as_deref().unwrap_or("No error message")
        ));
        if let Err(err) = sink.flush().await {
            nack_action_delivery(broker, consumer_id, delivery.delivery_id).await?;
            return Err(broker_error("publish_result", err));
        }

        let status = match result.status {
            runinator_models::runs::RunStatus::TimedOut => WorkflowStatus::TimedOut,
            runinator_models::runs::RunStatus::Canceled => WorkflowStatus::Canceled,
            _ => WorkflowStatus::Failed,
        };
        let output_json = TaskStatusOutput {
            success: false,
            duration_ms: Some(task_result.duration_ms()),
            message: provider_message.clone(),
        }
        .to_wire_value()?;
        if let Err(err) = sink
            .publish_status(status, Some(output_json), provider_message.clone())
            .await
        {
            error!(
                "Failed to publish workflow node run {} terminal status: {}",
                command.workflow_node_run_id, err
            );
            nack_action_delivery(broker, consumer_id, delivery.delivery_id).await?;
            return Err(broker_error("publish_result", err));
        }
        warn!(
            "Action {}.{} reported failure: {:?}",
            action.provider, action.function, provider_message
        );
    }

    broker
        .ack(consumer_id, delivery.delivery_id)
        .await
        .map_err(|err| broker_error("ack", err))
}

async fn nack_action_delivery(
    broker: &Arc<dyn Broker>,
    consumer_id: &str,
    delivery_id: uuid::Uuid,
) -> Result<(), SendableError> {
    broker
        .nack(consumer_id, delivery_id)
        .await
        .map_err(|err| broker_error("nack", err))
}
