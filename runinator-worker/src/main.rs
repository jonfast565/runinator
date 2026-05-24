mod config;
mod control_events;
mod executor;
mod output_sink;
mod provider_repository;

use std::{
    collections::{BTreeSet, HashMap},
    env,
    ffi::OsString,
    sync::Arc,
};

use config::parse_config;
use log::{error, info, warn};
use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_broker::{
    Broker, BrokerError, ControlDelivery, http::client::HttpBroker, in_memory::InMemoryBroker,
    tcp::client::TcpBroker,
};
use runinator_comm::ControlKind;
use runinator_comm::worker_control::WorkerControlEventKind;
use runinator_models::errors::{RuntimeError, SendableError};
use runinator_models::workflows::WorkflowStatus;
use runinator_plugin::{
    cancel::CancellationToken, load_libraries_from_path, plugin::Plugin, print_libs,
};
use runinator_utilities::startup;
use serde_json::json;
use tokio::{
    sync::{Mutex, Notify, Semaphore},
    task::JoinSet,
};

use crate::output_sink::RunOutputSink;
use control_events::{EventDetails, SchedulerControlClient};

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
    let broker = build_broker(&config)?;
    let api_client = build_api_client(&config)?;
    let control_client = Arc::new(SchedulerControlClient::new(&config)?);

    let shutdown = Arc::new(Notify::new());
    let mut worker_task = {
        let broker = broker.clone();
        let libraries = Arc::clone(&libraries);
        let api_client = api_client.clone();
        let control_client = Arc::clone(&control_client);
        let consumer = config.broker_consumer_id.clone();
        let max_concurrent_actions = config.max_concurrent_actions;
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            run_worker_loop(
                broker,
                consumer,
                libraries,
                api_client,
                control_client,
                max_concurrent_actions,
                shutdown,
            )
            .await
        })
    };

    tokio::select! {
        signal = tokio::signal::ctrl_c() => {
            signal.expect("Failed to listen for Ctrl+C");
            info!("Shutdown signal received. Stopping worker...");
            shutdown.notify_waiters();
        }
        result = &mut worker_task => {
            return handle_worker_task_result(result);
        }
    }

    if let Err(err) = worker_task.await {
        if !err.is_cancelled() {
            error!("Worker task join error: {}", err);
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

fn build_broker(config: &config::Config) -> Result<Arc<dyn Broker>, SendableError> {
    match config.broker_backend.as_str() {
        "http" => {
            let url = reqwest::Url::parse(&config.broker_endpoint).map_err(|err| {
                Box::new(RuntimeError::new(
                    "worker.broker.invalid_endpoint".into(),
                    err.to_string(),
                )) as SendableError
            })?;

            let client = reqwest::Client::builder()
                .build()
                .map_err(|err| -> SendableError {
                    Box::new(RuntimeError::new(
                        "worker.broker.client".into(),
                        err.to_string(),
                    ))
                })?;

            Ok(Arc::new(HttpBroker::new(url, client)))
        }
        "in-memory" => Ok(Arc::new(InMemoryBroker::new())),
        "tcp" => Ok(Arc::new(TcpBroker::new(config.broker_endpoint.clone()))),
        "rabbitmq" | "kafka" => Err(Box::new(RuntimeError::new(
            "worker.broker.backend_not_ready".into(),
            format!(
                "Broker backend '{}' is not implemented yet",
                config.broker_backend
            ),
        ))),
        other => Err(Box::new(RuntimeError::new(
            "worker.broker.unknown_backend".into(),
            format!("Unknown broker backend '{other}'"),
        ))),
    }
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

async fn run_worker_loop(
    broker: Arc<dyn Broker>,
    consumer_id: String,
    libraries: Arc<HashMap<String, Plugin>>,
    api_client: AsyncApiClient<StaticLocator>,
    control_client: Arc<SchedulerControlClient>,
    max_concurrent_actions: usize,
    shutdown: Arc<Notify>,
) -> Result<(), SendableError> {
    let max_concurrent_actions = max_concurrent_actions.max(1);
    let semaphore = Arc::new(Semaphore::new(max_concurrent_actions));
    let in_flight = Arc::new(Mutex::new(HashMap::<i64, CancellationToken>::new()));
    let control_task = tokio::spawn(run_control_loop(
        broker.clone(),
        consumer_id.clone(),
        Arc::clone(&control_client),
        Arc::clone(&in_flight),
        shutdown.clone(),
    ));
    let mut deliveries = JoinSet::new();
    info!("Worker processing up to {max_concurrent_actions} concurrent action(s)");
    send_control_event(
        &control_client,
        WorkerControlEventKind::WorkerStarted,
        EventDetails::empty(),
    )
    .await;

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
        let control_client = Arc::clone(&control_client);
        let in_flight = Arc::clone(&in_flight);
        deliveries.spawn(async move {
            let _permit = permit;
            if let Err(err) = process_delivery(
                &broker,
                &consumer_id,
                libraries,
                api_client,
                control_client,
                maybe_delivery,
                in_flight,
            )
            .await
            {
                error!("Error processing task: {}", err);
            }
        });
    }

    while let Some(result) = deliveries.join_next().await {
        if let Err(err) = result {
            error!("Worker delivery task join error: {}", err);
        }
    }

    match control_task.await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => error!("Worker control loop terminated with error: {}", err),
        Err(err) if err.is_cancelled() => {}
        Err(err) => error!("Worker control task join error: {}", err),
    }

    send_control_event(
        &control_client,
        WorkerControlEventKind::WorkerStopping,
        EventDetails::empty(),
    )
    .await;

    Ok(())
}

async fn run_control_loop(
    broker: Arc<dyn Broker>,
    consumer_id: String,
    control_client: Arc<SchedulerControlClient>,
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
        handle_control_delivery(&broker, &consumer_id, &control_client, &in_flight, delivery)
            .await?;
    }
}

async fn handle_control_delivery(
    broker: &Arc<dyn Broker>,
    consumer_id: &str,
    control_client: &Arc<SchedulerControlClient>,
    in_flight: &Arc<Mutex<HashMap<i64, CancellationToken>>>,
    delivery: ControlDelivery,
) -> Result<(), SendableError> {
    let control_kind = delivery.command.kind.clone();
    match control_kind.clone() {
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
                "Pause control received for workflow run {}; scheduler will stop dispatching at the next boundary",
                delivery.command.workflow_run_id
            );
        }
        ControlKind::Resume => {
            info!(
                "Resume control received for workflow run {}; scheduler controls dispatch resumption",
                delivery.command.workflow_run_id
            );
        }
    }
    send_control_event(
        control_client,
        WorkerControlEventKind::ControlApplied,
        EventDetails::for_control(
            delivery.command.workflow_run_id,
            control_kind,
            "Broker control applied by worker",
        ),
    )
    .await;
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
    control_client: Arc<SchedulerControlClient>,
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
    send_control_event(
        &control_client,
        WorkerControlEventKind::ActionStarted,
        EventDetails::for_action(
            command.workflow_run_id,
            command.workflow_node_run_id,
            command.node_id.clone(),
            "Action started",
        ),
    )
    .await;
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
            let output_json = json!({
                "success": false,
                "message": message,
            });
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
            send_control_event(
                &control_client,
                WorkerControlEventKind::ActionFinished,
                EventDetails::for_action(
                    command.workflow_run_id,
                    command.workflow_node_run_id,
                    command.node_id.clone(),
                    message,
                ),
            )
            .await;
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
    if let Some(execution_result) = &result.execution_result {
        if let Err(err) = sink.persist_result(execution_result).await {
            error!(
                "Failed to publish workflow node run {} result artifacts: {}",
                command.workflow_node_run_id, err
            );
            nack_action_delivery(broker, consumer_id, delivery.delivery_id).await?;
            return Err(broker_error("publish_result", err));
        }
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
            .unwrap_or_else(|| {
                json!({
                    "success": true,
                    "duration_ms": task_result.duration_ms(),
                    "message": provider_message,
                })
            });
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
        let output_json = json!({
            "success": false,
            "duration_ms": task_result.duration_ms(),
            "message": provider_message,
        });
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

    send_control_event(
        &control_client,
        WorkerControlEventKind::ActionFinished,
        EventDetails::for_action(
            command.workflow_run_id,
            command.workflow_node_run_id,
            command.node_id,
            provider_message.unwrap_or_else(|| "Action finished".into()),
        ),
    )
    .await;

    broker
        .ack(consumer_id, delivery.delivery_id)
        .await
        .map_err(|err| broker_error("ack", err))
}

async fn send_control_event(
    control_client: &SchedulerControlClient,
    kind: WorkerControlEventKind,
    details: EventDetails,
) {
    if let Err(err) = control_client.send(kind, details).await {
        warn!("Unable to send scheduler control event: {}", err);
    }
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

async fn resolve_secret_refs(
    api_client: &AsyncApiClient<StaticLocator>,
    parameters: serde_json::Value,
) -> Result<serde_json::Value, SendableError> {
    let mut refs = BTreeSet::new();
    collect_secret_refs(&parameters, &mut refs);
    if refs.is_empty() {
        return Ok(parameters);
    }

    let mut secrets = HashMap::new();
    for secret_ref in refs {
        let secret = api_client
            .fetch_credential(&secret_ref.scope, &secret_ref.name)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        secrets.insert(secret_ref, secret);
    }

    Ok(replace_secret_refs(parameters, &secrets))
}

fn collect_secret_refs(value: &serde_json::Value, refs: &mut BTreeSet<SecretRef>) {
    match value {
        serde_json::Value::String(raw) => {
            if let Some(secret_ref) = parse_secret_ref(raw) {
                refs.insert(secret_ref);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                collect_secret_refs(value, refs);
            }
        }
        serde_json::Value::Object(object) => {
            for value in object.values() {
                collect_secret_refs(value, refs);
            }
        }
        _ => {}
    }
}

fn replace_secret_refs(
    value: serde_json::Value,
    secrets: &HashMap<SecretRef, String>,
) -> serde_json::Value {
    match value {
        serde_json::Value::String(raw) => parse_secret_ref(&raw)
            .and_then(|secret_ref| secrets.get(&secret_ref).cloned())
            .map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::String(raw)),
        serde_json::Value::Array(values) => serde_json::Value::Array(
            values
                .into_iter()
                .map(|value| replace_secret_refs(value, secrets))
                .collect(),
        ),
        serde_json::Value::Object(object) => serde_json::Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (key, replace_secret_refs(value, secrets)))
                .collect(),
        ),
        other => other,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct SecretRef {
    scope: String,
    name: String,
}

fn parse_secret_ref(raw: &str) -> Option<SecretRef> {
    let path = raw.strip_prefix("secret://")?;
    let (scope, name) = path.split_once('/')?;
    if scope.is_empty() || name.is_empty() {
        return None;
    }
    Some(SecretRef {
        scope: percent_decode(scope)?,
        name: percent_decode(name)?,
    })
}

fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hi = hex_value(*bytes.get(index + 1)?)?;
            let lo = hex_value(*bytes.get(index + 2)?)?;
            decoded.push((hi << 4) | lo);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn broker_error(context: &'static str, err: BrokerError) -> SendableError {
    Box::new(RuntimeError::new(
        format!("worker.broker.{context}"),
        err.to_string(),
    ))
}
