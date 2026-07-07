use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use chrono::Utc;
use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_broker::{Broker, BrokerDelivery, ControlDelivery};
use runinator_comm::{ConsumerProfile, ControlKind, WireCodec};
use runinator_models::errors::{SendableError, error_code_or_unknown};
use runinator_models::workflow_state::TaskStatusOutput;
use runinator_models::workflows::WorkflowStatus;
use runinator_plugin::{
    cancel::CancellationToken, load_libraries_from_path, plugin::Plugin, print_libs,
};
use tokio::{
    sync::{Mutex, Notify, Semaphore},
    task::JoinSet,
};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::broker::broker_error;
use crate::events::{ActionOutcome, WorkerEvent, WorkerEventSink};
use crate::executor;
use crate::metrics;
use crate::output_sink::RunOutputSink;
use crate::provider_repository::ProviderFactory;
use crate::secrets::{is_transient_secret_error, resolve_secret_refs};

// grace added to an action's timeout before its executor lease is considered abandoned, so a worker
// that is merely slow (clock skew, a long flush) is never preempted by a duplicate delivery.
const EXECUTOR_LEASE_GRACE_SECONDS: i64 = 60;

// backoff before retrying a failed broker receive. a transient broker error (restart, network blip)
// must not tear down the loops: exiting the action loop aborts in-flight actions without
// cancellation or drain, and exiting the control loop silently disables cancellation.
const RECEIVE_RETRY_BACKOFF: Duration = Duration::from_secs(1);

// backoff before returning a delivery whose secrets could not be fetched from the web service, so
// a ws outage does not hot-loop claim/execute/nack cycles against the broker.
const SECRET_RETRY_BACKOFF: Duration = Duration::from_secs(5);

// one in-flight action execution, tracked so a control command can cancel it. the owning run id is
// retained so a run-wide cancel can fan out to every node run of that run.
#[derive(Clone)]
struct InFlightAction {
    workflow_run_id: Uuid,
    token: CancellationToken,
    // set by the control loop before it cancels the token, so the result path can tell a genuine
    // (ws-requested) cancel from a shutdown preemption that should requeue the delivery instead.
    canceled_by_control: Arc<AtomicBool>,
}

/// everything the action loop needs to run. assembled by the binary (or an embedded host such as the
/// desktop) and handed to [`start_worker_loop`].
pub struct WorkerRuntime {
    pub broker: Arc<dyn Broker>,
    pub profile: ConsumerProfile,
    pub libraries: Arc<HashMap<String, Plugin>>,
    pub api_client: AsyncApiClient<StaticLocator>,
    pub replica_id: Option<Uuid>,
    pub providers: ProviderFactory,
    pub max_concurrent_actions: usize,
    pub shutdown_grace: Duration,
    pub shutdown: Arc<Notify>,
    /// observer for loop activity; use [`crate::events::NoopEventSink`] when nothing listens.
    pub events: Arc<dyn WorkerEventSink>,
}

/// load plugin libraries from the supplied search paths, skipping any that do not exist.
pub fn load_libraries(paths: &[String]) -> Result<HashMap<String, Plugin>, SendableError> {
    let mut libraries = HashMap::new();
    for path in paths {
        if !std::path::Path::new(path).exists() {
            info!(path = %path, "skipping missing plugin path");
            continue;
        }

        info!(path = %path, "loading plugins");
        libraries.extend(load_libraries_from_path(path)?);
    }
    print_libs(&libraries);
    Ok(libraries)
}

/// run the action and control loops until `shutdown` is notified, draining in-flight work within the
/// configured grace period. caller owns signal handling; this never installs a signal handler.
pub async fn start_worker_loop(runtime: WorkerRuntime) -> Result<(), SendableError> {
    let WorkerRuntime {
        broker,
        profile,
        libraries,
        api_client,
        replica_id,
        providers,
        max_concurrent_actions,
        shutdown_grace,
        shutdown,
        events,
    } = runtime;

    // the ack channels are keyed by the consumer id; the action and control channels route by
    // profile. the control profile is never exclusive: exclusivity keeps a desktop worker from
    // stealing general-pool *work*, but a run-wide (untargeted) control must still reach it.
    let consumer_id = profile.id.clone();
    let control_profile = ConsumerProfile {
        exclusive: false,
        ..profile.clone()
    };
    let max_concurrent_actions = max_concurrent_actions.max(1);
    let semaphore = Arc::new(Semaphore::new(max_concurrent_actions));
    // keyed by node-run id so concurrent node runs of the same workflow run (parallel/race/map child
    // work) each get their own cancellation token; a targeted cancel reaches exactly one branch.
    let in_flight = Arc::new(Mutex::new(HashMap::<Uuid, InFlightAction>::new()));
    let control_task = tokio::spawn(run_control_loop(
        broker.clone(),
        control_profile,
        Arc::clone(&in_flight),
        shutdown.clone(),
        Arc::clone(&events),
    ));
    let mut deliveries = JoinSet::new();
    info!(max_concurrent_actions, "worker action loop started");

    loop {
        let permit = tokio::select! {
            biased;
            _ = shutdown.notified() => {
                info!("worker loop shutting down");
                break;
            }
            Some(result) = deliveries.join_next(), if !deliveries.is_empty() => {
                if let Err(err) = result {
                    error!("worker delivery task join error: {}", err);
                }
                continue;
            }
            permit = semaphore.clone().acquire_owned() => {
                permit.map_err(|err| crate::errors::CONCURRENCY_CLOSED.error(err))?
            }
        };

        let maybe_delivery = tokio::select! {
            _ = shutdown.notified() => {
                drop(permit);
                info!("worker loop shutting down");
                break;
            }
            result = broker.receive_for(&profile) => {
                match result {
                    Ok(delivery) => delivery,
                    Err(err) => {
                        drop(permit);
                        error!(
                            error_code = error_code_or_unknown(&err),
                            "failed to receive action delivery: {}", err
                        );
                        tokio::select! {
                            _ = shutdown.notified() => {
                                info!("worker loop shutting down");
                                break;
                            }
                            _ = tokio::time::sleep(RECEIVE_RETRY_BACKOFF) => {}
                        }
                        continue;
                    }
                }
            }
        };

        let trace_id = maybe_delivery.command.trace_id;
        let run_id = maybe_delivery.command.workflow_run_id;
        let node_id = maybe_delivery.command.node_id.clone();
        let broker = broker.clone();
        let consumer_id = consumer_id.clone();
        let libraries = Arc::clone(&libraries);
        let api_client = api_client.clone();
        let providers = Arc::clone(&providers);
        let replica_id = replica_id;
        let in_flight = Arc::clone(&in_flight);
        let events = Arc::clone(&events);
        deliveries.spawn(async move {
            let _permit = permit;
            if let Err(err) = process_delivery(
                &broker,
                &consumer_id,
                libraries,
                api_client,
                providers,
                replica_id,
                maybe_delivery,
                in_flight,
                events,
            )
            .await
            {
                error!(
                    trace_id = %trace_id,
                    run_id = %run_id,
                    node_id = %node_id,
                    error_code = error_code_or_unknown(err.as_ref()),
                    "error processing task: {}",
                    err
                );
            }
        });
    }

    cancel_in_flight(&in_flight).await;
    match tokio::time::timeout(shutdown_grace, drain_deliveries(&mut deliveries)).await {
        Ok(()) => {}
        Err(_) => {
            warn!(
                shutdown_grace_secs = shutdown_grace.as_secs(),
                "worker shutdown grace period elapsed; aborting unfinished action tasks"
            );
            deliveries.abort_all();
            drain_deliveries(&mut deliveries).await;
        }
    }

    match control_task.await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => error!("worker control loop terminated with error: {}", err),
        Err(err) if err.is_cancelled() => {}
        Err(err) => error!("worker control task join error: {}", err),
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
        count = actions.len(),
        "canceling in-flight action(s) during worker shutdown"
    );
    for action in actions {
        action.token.cancel();
    }
}

async fn drain_deliveries(deliveries: &mut JoinSet<()>) {
    while let Some(result) = deliveries.join_next().await {
        if let Err(err) = result {
            error!("worker delivery task join error: {}", err);
        }
    }
}

async fn run_control_loop(
    broker: Arc<dyn Broker>,
    profile: ConsumerProfile,
    in_flight: Arc<Mutex<HashMap<Uuid, InFlightAction>>>,
    shutdown: Arc<Notify>,
    events: Arc<dyn WorkerEventSink>,
) -> Result<(), SendableError> {
    let consumer_id = profile.id.clone();
    loop {
        let delivery = tokio::select! {
            _ = shutdown.notified() => {
                info!("Worker control loop shutting down");
                return Ok(());
            }
            // the targeting-aware path: a cancel stamped with another replica's id is never handed
            // to this worker, so it cannot be acked here and lost before reaching its holder.
            result = broker.receive_control_for(&profile) => {
                match result {
                    Ok(delivery) => delivery,
                    Err(err) => {
                        error!(
                            error_code = error_code_or_unknown(&err),
                            "failed to receive control command: {}", err
                        );
                        tokio::select! {
                            _ = shutdown.notified() => {
                                info!("Worker control loop shutting down");
                                return Ok(());
                            }
                            _ = tokio::time::sleep(RECEIVE_RETRY_BACKOFF) => {}
                        }
                        continue;
                    }
                }
            }
        };
        // an ack failure is transient: the broker lease redelivers the control, and handling one
        // twice is harmless. keep the loop alive so cancellation is never silently disabled.
        if let Err(err) =
            handle_control_delivery(&broker, &consumer_id, &in_flight, &events, delivery).await
        {
            error!(
                error_code = error_code_or_unknown(err.as_ref()),
                "failed to handle control delivery: {}", err
            );
        }
    }
}

async fn handle_control_delivery(
    broker: &Arc<dyn Broker>,
    consumer_id: &str,
    in_flight: &Arc<Mutex<HashMap<Uuid, InFlightAction>>>,
    events: &Arc<dyn WorkerEventSink>,
    delivery: ControlDelivery,
) -> Result<(), SendableError> {
    let control_kind = delivery.command.kind;
    metrics::control_command(match control_kind {
        ControlKind::Cancel => "cancel",
        ControlKind::Pause => "pause",
        ControlKind::Resume => "resume",
    });
    events.handle(WorkerEvent::ControlReceived {
        kind: control_kind,
        workflow_run_id: delivery.command.workflow_run_id,
    });
    match control_kind {
        ControlKind::Cancel => {
            // a node-run-targeted cancel reaches exactly one losing race branch; a run-wide cancel
            // fans out to every node run of the run held on this worker.
            let actions = {
                let guard = in_flight.lock().await;
                match delivery.command.workflow_node_run_id {
                    Some(node_run_id) => guard
                        .get(&node_run_id)
                        .cloned()
                        .into_iter()
                        .collect::<Vec<_>>(),
                    None => guard
                        .values()
                        .filter(|action| action.workflow_run_id == delivery.command.workflow_run_id)
                        .cloned()
                        .collect::<Vec<_>>(),
                }
            };
            if actions.is_empty() {
                info!(
                    run_id = %delivery.command.workflow_run_id,
                    node_id = ?delivery.command.workflow_node_run_id,
                    "cancellation requested, but no matching local execution is active"
                );
            } else {
                for action in &actions {
                    // flag before canceling so the result path never observes a control-canceled
                    // token without the flag.
                    action.canceled_by_control.store(true, Ordering::Release);
                    action.token.cancel();
                }
                info!(
                    run_id = %delivery.command.workflow_run_id,
                    node_id = ?delivery.command.workflow_node_run_id,
                    canceled = actions.len(),
                    "cancellation requested; canceled local execution(s)"
                );
            }
        }
        ControlKind::Pause => {
            info!(
                run_id = %delivery.command.workflow_run_id,
                "pause control received; the web service will stop dispatching at the next boundary"
            );
        }
        ControlKind::Resume => {
            info!(
                run_id = %delivery.command.workflow_run_id,
                "resume control received; the web service controls dispatch resumption"
            );
        }
    }
    broker
        .ack_control(consumer_id, delivery.delivery_id)
        .await
        .map_err(|err| broker_error("ack_control", err))
}

#[allow(clippy::too_many_arguments)]
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
    providers: ProviderFactory,
    replica_id: Option<Uuid>,
    delivery: BrokerDelivery,
    in_flight: Arc<Mutex<HashMap<Uuid, InFlightAction>>>,
    events: Arc<dyn WorkerEventSink>,
) -> Result<(), SendableError> {
    // link this execution span to the trace that dispatched the action (w3c context from the broker
    // message). a no-op when the dispatcher had otel off.
    runinator_utilities::telemetry::apply_trace_context(
        &tracing::Span::current(),
        &delivery.command.trace_context,
    );
    metrics::action_received();
    let command = delivery.command.clone();
    let action = command.action.clone();
    let token = CancellationToken::new();
    in_flight.lock().await.insert(
        command.workflow_node_run_id,
        InFlightAction {
            workflow_run_id: command.workflow_run_id,
            token: token.clone(),
            canceled_by_control: Arc::new(AtomicBool::new(false)),
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
                    node_run_id = %command.workflow_node_run_id,
                    "skipping duplicate delivery: executor lease held elsewhere"
                );
                metrics::action_duplicate();
                events.handle(WorkerEvent::ActionSkippedDuplicate {
                    node_run_id: command.workflow_node_run_id,
                });
                in_flight.lock().await.remove(&command.workflow_node_run_id);
                broker
                    .ack(consumer_id, delivery.delivery_id)
                    .await
                    .map_err(|err| broker_error("ack", err))?;
                return Ok(());
            }
            // fail-open on a transport error so a transient ws outage cannot wedge execution.
            Err(err) => warn!(
                replica_id = %replica_id,
                node_run_id = %command.workflow_node_run_id,
                "failed to claim executor: {}",
                err
            ),
        }
    }
    events.handle(WorkerEvent::ActionStarted {
        workflow_run_id: command.workflow_run_id,
        node_id: command.node_id.clone(),
        node_run_id: command.workflow_node_run_id,
        provider: action.provider.clone(),
        function: action.function.clone(),
        attempt: command.attempt,
    });
    if let Err(err) = sink
        .publish_status(WorkflowStatus::Running, None, None)
        .await
    {
        error!(
            node_run_id = %command.workflow_node_run_id,
            "failed to publish running status: {}",
            err
        );
        in_flight.lock().await.remove(&command.workflow_node_run_id);
        nack_action_delivery(
            broker,
            consumer_id,
            &api_client,
            replica_id,
            command.workflow_node_run_id,
            delivery.delivery_id,
        )
        .await?;
        return Err(broker_error("publish_result", err));
    }
    let parameters = match resolve_secret_refs(&api_client, command.parameters.clone()).await {
        Ok(parameters) => parameters,
        // a transport failure or web-service outage is transient: the secret may resolve fine in a
        // moment, so return the delivery for redelivery instead of failing the node (the default
        // retry policy gives a node one attempt, so a ws blip would otherwise fail the whole run).
        Err(err) if is_transient_secret_error(&err) => {
            warn!(
                node_run_id = %command.workflow_node_run_id,
                error_code = error_code_or_unknown(err.as_ref()),
                "transient failure resolving action secrets; returning delivery for retry: {}",
                err
            );
            metrics::secret_resolution_failure();
            in_flight.lock().await.remove(&command.workflow_node_run_id);
            // pause so an unreachable web service does not hot-loop claim/nack cycles.
            tokio::time::sleep(SECRET_RETRY_BACKOFF).await;
            return nack_action_delivery(
                broker,
                consumer_id,
                &api_client,
                replica_id,
                command.workflow_node_run_id,
                delivery.delivery_id,
            )
            .await;
        }
        Err(err) => {
            let message = format!("Failed to resolve action secrets: {err}");
            error!(
                node_run_id = %command.workflow_node_run_id,
                error_code = error_code_or_unknown(err.as_ref()),
                "{}",
                message
            );
            metrics::secret_resolution_failure();
            events.handle(WorkerEvent::ActionFinished {
                workflow_run_id: command.workflow_run_id,
                node_id: command.node_id.clone(),
                node_run_id: command.workflow_node_run_id,
                provider: action.provider.clone(),
                function: action.function.clone(),
                outcome: ActionOutcome::Failed,
                duration_ms: 0,
                message: Some(message.clone()),
            });
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
                    node_run_id = %command.workflow_node_run_id,
                    error_code = error_code_or_unknown(&err),
                    "failed to publish failed status: {}",
                    err
                );
                in_flight.lock().await.remove(&command.workflow_node_run_id);
                nack_action_delivery(
                    broker,
                    consumer_id,
                    &api_client,
                    replica_id,
                    command.workflow_node_run_id,
                    delivery.delivery_id,
                )
                .await?;
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
    let result = {
        // raise the in-flight gauge only around actual execution, so it reflects running providers
        // rather than deliveries parked on lease/secret checks.
        let _in_flight = metrics::in_flight_guard();
        executor::execute_task(
            &providers,
            libraries,
            action.clone(),
            command.workflow_node_run_id,
            parameters,
            Some(Arc::new(sink.clone())),
            token,
        )
        .await
    };
    let finished = in_flight.lock().await.remove(&command.workflow_node_run_id);
    let outcome = match result.status {
        runinator_models::runs::RunStatus::TimedOut => ActionOutcome::TimedOut,
        runinator_models::runs::RunStatus::Canceled => ActionOutcome::Canceled,
        _ if result.task_result.success => ActionOutcome::Succeeded,
        _ => ActionOutcome::Failed,
    };
    // a cancellation no control command requested is shutdown preemption: the workflow itself was
    // not canceled, so return the delivery (and the executor lease) for redelivery on another
    // worker instead of publishing a terminal status that would settle the node — and with the
    // default no-retry policy, the run — as canceled by a mere rolling restart. the mapped outcome
    // races between `Canceled` (the executor's cancel arm) and `Failed` (a token-honoring provider
    // returning an error first), so the preemption signal is the token itself: cancelled, but not
    // by a control command, and not by the executor's own timeout (that maps to `TimedOut`).
    let canceled_by_control = finished
        .as_ref()
        .is_some_and(|action| action.canceled_by_control.load(Ordering::Acquire));
    if matches!(outcome, ActionOutcome::Canceled | ActionOutcome::Failed)
        && finished
            .as_ref()
            .is_some_and(|action| action.token.is_cancelled())
        && !canceled_by_control
    {
        warn!(
            node_run_id = %command.workflow_node_run_id,
            "action preempted by worker shutdown; returning delivery for redelivery"
        );
        metrics::action_completed("requeued", result.task_result.duration_ms() as f64);
        return nack_action_delivery(
            broker,
            consumer_id,
            &api_client,
            replica_id,
            command.workflow_node_run_id,
            delivery.delivery_id,
        )
        .await;
    }
    metrics::action_completed(outcome.as_str(), result.task_result.duration_ms() as f64);
    if let Some(execution_result) = &result.execution_result
        && let Err(err) = sink.persist_result(execution_result).await
    {
        error!(
            node_run_id = %command.workflow_node_run_id,
            error_code = error_code_or_unknown(&err),
            "failed to publish result artifacts: {}",
            err
        );
        nack_action_delivery(
            broker,
            consumer_id,
            &api_client,
            replica_id,
            command.workflow_node_run_id,
            delivery.delivery_id,
        )
        .await?;
        return Err(broker_error("publish_result", err));
    }
    let task_result = result.task_result;
    let provider_message = task_result.message.clone().or_else(|| sink.message());
    events.handle(WorkerEvent::ActionFinished {
        workflow_run_id: command.workflow_run_id,
        node_id: command.node_id.clone(),
        node_run_id: command.workflow_node_run_id,
        provider: action.provider.clone(),
        function: action.function.clone(),
        outcome,
        duration_ms: task_result.duration_ms(),
        message: provider_message.clone(),
    });

    if task_result.success {
        info!(
            node_run_id = %command.workflow_node_run_id,
            provider = %action.provider,
            function = %action.function,
            duration_ms = task_result.duration_ms(),
            "action completed successfully"
        );
        sink.emit_log(format!(
            "Action {}.{} completed successfully in {} ms.",
            action.provider,
            action.function,
            task_result.duration_ms()
        ));
        if let Err(err) = sink.flush().await {
            nack_action_delivery(
                broker,
                consumer_id,
                &api_client,
                replica_id,
                command.workflow_node_run_id,
                delivery.delivery_id,
            )
            .await?;
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
                node_run_id = %command.workflow_node_run_id,
                error_code = error_code_or_unknown(&err),
                "failed to publish succeeded status: {}",
                err
            );
            nack_action_delivery(
                broker,
                consumer_id,
                &api_client,
                replica_id,
                command.workflow_node_run_id,
                delivery.delivery_id,
            )
            .await?;
            return Err(broker_error("publish_result", err));
        }
    } else {
        warn!(
            node_run_id = %command.workflow_node_run_id,
            provider = %action.provider,
            function = %action.function,
            duration_ms = task_result.duration_ms(),
            message = provider_message.as_deref().unwrap_or("No error message"),
            "action failed"
        );
        sink.emit_log(format!(
            "Action {}.{} failed after {} ms: {}.",
            action.provider,
            action.function,
            task_result.duration_ms(),
            provider_message.as_deref().unwrap_or("No error message")
        ));
        if let Err(err) = sink.flush().await {
            nack_action_delivery(
                broker,
                consumer_id,
                &api_client,
                replica_id,
                command.workflow_node_run_id,
                delivery.delivery_id,
            )
            .await?;
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
                node_run_id = %command.workflow_node_run_id,
                error_code = error_code_or_unknown(&err),
                "failed to publish terminal status: {}",
                err
            );
            nack_action_delivery(
                broker,
                consumer_id,
                &api_client,
                replica_id,
                command.workflow_node_run_id,
                delivery.delivery_id,
            )
            .await?;
            return Err(broker_error("publish_result", err));
        }
    }

    broker
        .ack(consumer_id, delivery.delivery_id)
        .await
        .map_err(|err| broker_error("ack", err))?;
    // release the executor lease only after the ack commits the delivery: releasing first would
    // let an ack failure redeliver this already-completed action into a free claim and re-run its
    // side effects. a failed release self-heals once the claim ages past the staleness deadline.
    if let Some(replica_id) = replica_id {
        let _ = api_client
            .release_workflow_node_run_executor(
                command.workflow_node_run_id,
                replica_id,
                Utc::now(),
            )
            .await;
    }
    Ok(())
}

/// return a delivery to the broker for redelivery, releasing this worker's executor lease first.
/// without the release the retry is lost: the executor claim is not re-entrant (not even for this
/// replica), so every redelivery would be dropped as a duplicate and acked until the lease goes
/// stale, parking the node run until the reducer's timeout backstop fires.
async fn nack_action_delivery(
    broker: &Arc<dyn Broker>,
    consumer_id: &str,
    api_client: &AsyncApiClient<StaticLocator>,
    replica_id: Option<Uuid>,
    node_run_id: Uuid,
    delivery_id: uuid::Uuid,
) -> Result<(), SendableError> {
    if let Some(replica_id) = replica_id {
        // best-effort: a failed release still self-heals once the lease ages past the deadline.
        if let Err(err) = api_client
            .release_workflow_node_run_executor(node_run_id, replica_id, Utc::now())
            .await
        {
            warn!(
                node_run_id = %node_run_id,
                "failed to release executor lease before redelivery: {}",
                err
            );
        }
    }
    broker
        .nack(consumer_id, delivery_id)
        .await
        .map_err(|err| broker_error("nack", err))
}
