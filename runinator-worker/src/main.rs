mod config;
mod console_provider;
mod executor;
mod output_sink;
mod provider_repository;

use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use config::parse_config;
use log::{error, info, warn};
use runinator_api::{AsyncApiClient, StaticLocator, WorkflowNodeRunStatusPayload};
use runinator_broker::{
    Broker, BrokerError, http::client::HttpBroker, in_memory::InMemoryBroker,
    tcp::client::TcpBroker,
};
use runinator_models::errors::{RuntimeError, SendableError};
use runinator_models::workflows::WorkflowStatus;
use runinator_plugin::{load_libraries_from_path, plugin::Plugin, print_libs};
use runinator_utilities::startup;
use serde_json::json;
use tokio::{
    sync::{Notify, Semaphore},
    task::JoinSet,
};

use crate::output_sink::RunOutputSink;

#[tokio::main]
async fn main() -> Result<(), SendableError> {
    startup::startup("Runinator Worker")?;

    let config = parse_config()?;
    info!("Worker ID: {}", config.worker_id);

    let libraries = Arc::new(load_libraries(&config.dll_path)?);
    let broker = build_broker(&config)?;
    let api_client = build_api_client(&config)?;
    publish_provider_metadata(&api_client, &libraries).await;

    let shutdown = Arc::new(Notify::new());
    let mut worker_task = {
        let broker = broker.clone();
        let libraries = Arc::clone(&libraries);
        let api_client = api_client.clone();
        let consumer = config.broker_consumer_id.clone();
        let max_concurrent_actions = config.max_concurrent_actions;
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            run_worker_loop(
                broker,
                consumer,
                libraries,
                api_client,
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

fn load_libraries(path: &str) -> Result<HashMap<String, Plugin>, SendableError> {
    info!("Loading plugins from {}", path);
    let libraries = load_libraries_from_path(path)?;
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

async fn publish_provider_metadata(
    api_client: &AsyncApiClient<StaticLocator>,
    libraries: &HashMap<String, Plugin>,
) {
    for provider in provider_repository::provider_metadata(libraries) {
        match api_client.upsert_provider(&provider).await {
            Ok(_) => info!("Registered provider metadata for {}", provider.name),
            Err(err) => warn!(
                "Failed to register provider metadata for {}: {}",
                provider.name, err
            ),
        }
    }
}

async fn run_worker_loop(
    broker: Arc<dyn Broker>,
    consumer_id: String,
    libraries: Arc<HashMap<String, Plugin>>,
    api_client: AsyncApiClient<StaticLocator>,
    max_concurrent_actions: usize,
    shutdown: Arc<Notify>,
) -> Result<(), SendableError> {
    let max_concurrent_actions = max_concurrent_actions.max(1);
    let semaphore = Arc::new(Semaphore::new(max_concurrent_actions));
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
        deliveries.spawn(async move {
            let _permit = permit;
            if let Err(err) =
                process_delivery(&broker, &consumer_id, libraries, api_client, maybe_delivery).await
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

    Ok(())
}

async fn process_delivery(
    broker: &Arc<dyn Broker>,
    consumer_id: &str,
    libraries: Arc<HashMap<String, Plugin>>,
    api_client: AsyncApiClient<StaticLocator>,
    delivery: runinator_broker::BrokerDelivery,
) -> Result<(), SendableError> {
    let command = delivery.command.clone();
    let action = command.action.clone();
    let payload = WorkflowNodeRunStatusPayload {
        status: WorkflowStatus::Running,
        output_json: None,
        message: None,
    };
    if let Err(err) = api_client
        .set_workflow_node_run_status(command.workflow_node_run_id, &payload)
        .await
    {
        error!(
            "Failed to mark workflow node run {} running: {}",
            command.workflow_node_run_id, err
        );
    }
    let parameters = match resolve_secret_refs(&api_client, command.parameters.clone()).await {
        Ok(parameters) => parameters,
        Err(err) => {
            let message = format!("Failed to resolve action secrets: {err}");
            error!("{}", message);
            let payload = WorkflowNodeRunStatusPayload {
                status: WorkflowStatus::Failed,
                output_json: Some(json!({
                    "success": false,
                    "message": message,
                })),
                message: Some(message),
            };
            if let Err(err) = api_client
                .set_workflow_node_run_status(command.workflow_node_run_id, &payload)
                .await
            {
                error!(
                    "Failed to mark workflow node run {} failed: {}",
                    command.workflow_node_run_id, err
                );
            }
            broker
                .ack(consumer_id, delivery.delivery_id)
                .await
                .map_err(|err| broker_error("ack", err))?;
            return Ok(());
        }
    };
    let sink = RunOutputSink::new(
        command.workflow_node_run_id,
        api_client.clone(),
        tokio::runtime::Handle::current(),
    );
    let result = executor::execute_task(
        libraries,
        command.command_id,
        action.clone(),
        command.workflow_node_run_id,
        parameters,
        Some(Arc::new(sink.clone())),
    )
    .await;
    if let Some(execution_result) = &result.execution_result {
        sink.persist_result(execution_result).await;
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
        sink.flush().await;

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
        let payload = WorkflowNodeRunStatusPayload {
            status: WorkflowStatus::Succeeded,
            output_json: Some(output_json),
            message: provider_message.clone(),
        };
        if let Err(err) = api_client
            .set_workflow_node_run_status(command.workflow_node_run_id, &payload)
            .await
        {
            error!(
                "Failed to mark workflow node run {} succeeded: {}",
                command.workflow_node_run_id, err
            );
        }
    } else {
        sink.emit_log(format!(
            "Action {}.{} failed after {} ms: {}.",
            action.provider,
            action.function,
            task_result.duration_ms(),
            provider_message.as_deref().unwrap_or("No error message")
        ));
        sink.flush().await;

        let status = match result.status {
            runinator_models::runs::RunStatus::TimedOut => WorkflowStatus::TimedOut,
            runinator_models::runs::RunStatus::Canceled => WorkflowStatus::Canceled,
            _ => WorkflowStatus::Failed,
        };
        let payload = WorkflowNodeRunStatusPayload {
            status,
            output_json: Some(json!({
                "success": false,
                "duration_ms": task_result.duration_ms(),
                "message": provider_message,
            })),
            message: provider_message.clone(),
        };
        if let Err(err) = api_client
            .set_workflow_node_run_status(command.workflow_node_run_id, &payload)
            .await
        {
            error!(
                "Failed to mark workflow node run {} terminal: {}",
                command.workflow_node_run_id, err
            );
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
