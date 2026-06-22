mod broker;
mod config;
mod errors;
mod executor;
mod output_sink;
mod provider_repository;
mod secrets;

use std::{collections::HashMap, env, ffi::OsString, sync::Arc, time::Duration};
use uuid::Uuid;

use chrono::Utc;
use config::parse_config;
use log::{error, info, warn};
use runinator_api::{
    AsyncApiClient, ReplicaServiceConfig, ReplicaSession, StaticLocator, register_replica_session,
    spawn_replica_heartbeat,
};
use runinator_broker::{Broker, ControlDelivery};
use runinator_comm::ControlKind;
use runinator_comm::WireCodec;
use runinator_models::errors::SendableError;
use runinator_models::replicas::ReplicaKind;
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

// grace added to an action's timeout before its executor lease is considered abandoned, so a worker
// that is merely slow (clock skew, a long flush) is never preempted by a duplicate delivery.
const EXECUTOR_LEASE_GRACE_SECONDS: i64 = 60;
use broker::{broker_error, build_broker};
use secrets::resolve_secret_refs;

// one in-flight action execution, tracked so a control command can cancel it. the owning run id is
// retained so a run-wide cancel can fan out to every node run of that run.
#[derive(Clone)]
struct InFlightAction {
    workflow_run_id: Uuid,
    token: CancellationToken,
}

#[cfg(test)]
mod tests;

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

async fn run(config: config::Config) -> Result<(), SendableError> {
    info!("Worker ID: {}", config.worker_id);

    let libraries = Arc::new(load_libraries(&config.dll_paths)?);
    let broker = build_broker(&config).await?;
    let api_client = build_api_client(&config)?;
    let shutdown = Arc::new(Notify::new());

    if !config.liveness_file.trim().is_empty() {
        let path = config.liveness_file.clone();
        let liveness_shutdown = shutdown.clone();
        tokio::spawn(async move {
            loop {
                let _ = std::fs::write(&path, b"");
                tokio::select! {
                    _ = liveness_shutdown.notified() => return,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {}
                }
            }
        });
    }
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
        let broker = broker.clone();
        let libraries = Arc::clone(&libraries);
        let api_client = api_client.clone();
        let replica_id = replica_session.as_ref().map(ReplicaSession::replica_id);
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
                replica_id,
                max_concurrent_actions,
                shutdown_grace,
                shutdown,
            )
            .await
        })
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
            Err(errors::LOOP_JOIN.error(err))
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
    AsyncApiClient::with_credentials(locator, config.api_key.clone())
        .map_err(|err| errors::API_CLIENT.error(err))
}

async fn register_worker_replica(
    api_client: &AsyncApiClient<StaticLocator>,
    config: &config::Config,
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

#[allow(clippy::too_many_arguments)]
async fn run_worker_loop(
    broker: Arc<dyn Broker>,
    consumer_id: String,
    libraries: Arc<HashMap<String, Plugin>>,
    api_client: AsyncApiClient<StaticLocator>,
    replica_id: Option<Uuid>,
    max_concurrent_actions: usize,
    shutdown_grace: Duration,
    shutdown: Arc<Notify>,
) -> Result<(), SendableError> {
    let max_concurrent_actions = max_concurrent_actions.max(1);
    let semaphore = Arc::new(Semaphore::new(max_concurrent_actions));
    // keyed by node-run id so concurrent node runs of the same workflow run (parallel/race/map child
    // work) each get their own cancellation token; a targeted cancel reaches exactly one branch.
    let in_flight = Arc::new(Mutex::new(HashMap::<Uuid, InFlightAction>::new()));
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
                permit.map_err(|err| errors::CONCURRENCY_CLOSED.error(err))?
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
        let replica_id = replica_id;
        let in_flight = Arc::clone(&in_flight);
        deliveries.spawn(async move {
            let _permit = permit;
            if let Err(err) = process_delivery(
                &broker,
                &consumer_id,
                libraries,
                api_client,
                replica_id,
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

async fn cancel_in_flight(in_flight: &Arc<Mutex<HashMap<Uuid, InFlightAction>>>) {
    let actions = {
        let guard = in_flight.lock().await;
        guard.values().cloned().collect::<Vec<_>>()
    };
    if actions.is_empty() {
        return;
    }
    warn!(
        "Canceling {} in-flight action(s) during worker shutdown",
        actions.len()
    );
    for action in actions {
        action.token.cancel();
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
    in_flight: Arc<Mutex<HashMap<Uuid, InFlightAction>>>,
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
    in_flight: &Arc<Mutex<HashMap<Uuid, InFlightAction>>>,
    delivery: ControlDelivery,
) -> Result<(), SendableError> {
    let control_kind = delivery.command.kind;
    match control_kind {
        ControlKind::Cancel => {
            // a node-run-targeted cancel reaches exactly one losing race branch; a run-wide cancel
            // fans out to every node run of the run held on this worker.
            let tokens = {
                let guard = in_flight.lock().await;
                match delivery.command.workflow_node_run_id {
                    Some(node_run_id) => guard
                        .get(&node_run_id)
                        .map(|action| action.token.clone())
                        .into_iter()
                        .collect::<Vec<_>>(),
                    None => guard
                        .values()
                        .filter(|action| action.workflow_run_id == delivery.command.workflow_run_id)
                        .map(|action| action.token.clone())
                        .collect::<Vec<_>>(),
                }
            };
            if tokens.is_empty() {
                info!(
                    "Cancellation requested for workflow run {} (node run {:?}), but no matching local execution is active",
                    delivery.command.workflow_run_id, delivery.command.workflow_node_run_id
                );
            } else {
                for token in &tokens {
                    token.cancel();
                }
                info!(
                    "Cancellation requested for workflow run {} (node run {:?}); canceled {} local execution(s)",
                    delivery.command.workflow_run_id,
                    delivery.command.workflow_node_run_id,
                    tokens.len()
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

#[tracing::instrument(
    name = "execute_action",
    skip_all,
    fields(
        trace_id = %delivery.command.trace_id,
        run_id = %delivery.command.workflow_run_id,
        node_id = %delivery.command.node_id,
        attempt = delivery.command.attempt,
    )
)]
async fn process_delivery(
    broker: &Arc<dyn Broker>,
    consumer_id: &str,
    libraries: Arc<HashMap<String, Plugin>>,
    api_client: AsyncApiClient<StaticLocator>,
    replica_id: Option<Uuid>,
    delivery: runinator_broker::BrokerDelivery,
    in_flight: Arc<Mutex<HashMap<Uuid, InFlightAction>>>,
) -> Result<(), SendableError> {
    let command = delivery.command.clone();
    let action = command.action.clone();
    let token = CancellationToken::new();
    in_flight.lock().await.insert(
        command.workflow_node_run_id,
        InFlightAction {
            workflow_run_id: command.workflow_run_id,
            token: token.clone(),
        },
    );
    let sink = RunOutputSink::new(
        command.clone(),
        broker.clone(),
        tokio::runtime::Handle::current(),
    );
    // acquire the execution lease before anything observable runs. a redelivered or timeout-raced
    // duplicate of this node run loses the claim and is dropped here, so the action never executes
    // twice concurrently. the lease is treated as abandoned once it ages past the action's deadline.
    if let Some(replica_id) = replica_id {
        let stale_before = Utc::now()
            - chrono::Duration::seconds(action.timeout_seconds + EXECUTOR_LEASE_GRACE_SECONDS);
        match api_client
            .claim_workflow_node_run_executor(
                command.workflow_node_run_id,
                replica_id,
                Utc::now(),
                stale_before,
            )
            .await
        {
            Ok(true) => {}
            Ok(false) => {
                info!(
                    "Skipping duplicate delivery for node run {}: executor lease held elsewhere",
                    command.workflow_node_run_id
                );
                in_flight.lock().await.remove(&command.workflow_node_run_id);
                broker
                    .ack(consumer_id, delivery.delivery_id)
                    .await
                    .map_err(|err| broker_error("ack", err))?;
                return Ok(());
            }
            // fail-open on a transport error so a transient ws outage cannot wedge execution.
            Err(err) => warn!(
                "Failed to claim executor replica {} for node run {}: {}",
                replica_id, command.workflow_node_run_id, err
            ),
        }
    }
    if let Err(err) = sink
        .publish_status(WorkflowStatus::Running, None, None)
        .await
    {
        error!(
            "Failed to publish workflow node run {} running status: {}",
            command.workflow_node_run_id, err
        );
        in_flight.lock().await.remove(&command.workflow_node_run_id);
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
                in_flight.lock().await.remove(&command.workflow_node_run_id);
                nack_action_delivery(broker, consumer_id, delivery.delivery_id).await?;
                return Err(broker_error("publish_result", err));
            }
            broker
                .ack(consumer_id, delivery.delivery_id)
                .await
                .map_err(|err| broker_error("ack", err))?;
            if let Some(replica_id) = replica_id {
                let _ = api_client
                    .release_workflow_node_run_executor(
                        command.workflow_node_run_id,
                        replica_id,
                        Utc::now(),
                    )
                    .await;
            }
            in_flight.lock().await.remove(&command.workflow_node_run_id);
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
    in_flight.lock().await.remove(&command.workflow_node_run_id);
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
        if let Some(replica_id) = replica_id {
            let _ = api_client
                .release_workflow_node_run_executor(
                    command.workflow_node_run_id,
                    replica_id,
                    Utc::now(),
                )
                .await;
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
        if let Some(replica_id) = replica_id {
            let _ = api_client
                .release_workflow_node_run_executor(
                    command.workflow_node_run_id,
                    replica_id,
                    Utc::now(),
                )
                .await;
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
