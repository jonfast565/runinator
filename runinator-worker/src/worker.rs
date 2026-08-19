use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_broker::{Broker, BrokerDelivery, BrokerError, ControlDelivery};
use runinator_comm::{ActionCommand, ConsumerProfile, ControlKind, WireCodec};
use runinator_models::errors::{SendableError, error_code_or_unknown};
use runinator_models::workflow_state::TaskStatusOutput;
use runinator_models::workflows::{WorkflowAction, WorkflowStatus};
use runinator_plugin::{
    cancel::CancellationToken, load_libraries_from_path, plugin::Plugin, print_libs,
};
use tokio::{
    sync::{Mutex, Notify, Semaphore},
    task::JoinSet,
};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::agent::outbox::{ResultOutbox, drain_before_work, drain_forever};
use crate::agent::{
    DirectiveHandler,
    directives::{DirectiveLoopState, run_directive_loop},
};
use crate::broker::broker_error;
use crate::events::{ActionOutcome, WorkerEvent, WorkerEventSink};
use crate::executor;
use crate::function_cache::FunctionCache;
use crate::lease::ExecutorLeaseManager;
use crate::metrics;
use crate::output_sink::RunOutputSink;
use crate::provider_repository::ProviderFactory;
use crate::secrets::{is_transient_secret_error, resolve_secret_refs};

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
    pub result_outbox: Arc<dyn ResultOutbox>,
    pub directive_handler: Arc<dyn DirectiveHandler>,
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
        result_outbox,
        directive_handler,
    } = runtime;

    if !drain_before_work(result_outbox.as_ref(), broker.as_ref(), shutdown.as_ref()).await? {
        return Ok(());
    }
    let outbox_task = {
        let broker = broker.clone();
        let result_outbox = result_outbox.clone();
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            drain_forever(result_outbox.as_ref(), broker.as_ref(), shutdown.as_ref()).await
        })
    };

    // the ack channels are keyed by the consumer id; the action and control channels route by
    // profile. the control profile is never exclusive: exclusivity keeps a desktop worker from
    // stealing general-pool *work*, but a run-wide (untargeted) control must still reach it.
    let consumer_id = profile.id.clone();
    let control_profile = ConsumerProfile {
        exclusive: false,
        ..profile.clone()
    };
    let max_concurrent_actions = max_concurrent_actions.max(1);
    metrics::capacity(max_concurrent_actions);
    let semaphore = Arc::new(Semaphore::new(max_concurrent_actions));
    // keyed by node-run id so concurrent node runs of the same workflow run (parallel/race/map child
    // work) each get their own cancellation token; a targeted cancel reaches exactly one branch.
    let in_flight = Arc::new(Mutex::new(HashMap::<Uuid, InFlightAction>::new()));
    let drained = Arc::new(AtomicBool::new(false));
    let restart_requested = Arc::new(AtomicBool::new(false));
    let directive_state_changed = Arc::new(Notify::new());
    let executor_leases = ExecutorLeaseManager::new(api_client.clone(), replica_id);
    // one cache per worker, shared by every delivery: it is keyed by digest, so two concurrent
    // invocations of the same version stage one copy rather than racing to unpack two.
    let function_cache = Arc::new(FunctionCache::new(api_client.clone()));
    let control_task = tokio::spawn(run_control_loop(
        broker.clone(),
        control_profile,
        Arc::clone(&in_flight),
        shutdown.clone(),
        Arc::clone(&events),
    ));
    let directive_task = tokio::spawn(run_directive_loop(
        broker.clone(),
        profile.clone(),
        directive_handler,
        DirectiveLoopState {
            drained: Arc::clone(&drained),
            restart_requested: Arc::clone(&restart_requested),
            state_changed: Arc::clone(&directive_state_changed),
        },
        shutdown.clone(),
    ));
    let mut deliveries = JoinSet::new();
    let mut loop_error = None;
    info!(max_concurrent_actions, "worker action loop started");

    loop {
        if restart_requested.load(Ordering::SeqCst) {
            info!("agent restart directive received");
            break;
        }
        if drained.load(Ordering::SeqCst) {
            tokio::select! {
                _ = shutdown.notified() => break,
                _ = directive_state_changed.notified() => continue,
                _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
            }
        }
        if result_outbox.is_full() {
            warn!(
                outbox_depth = result_outbox.depth(),
                "result outbox is full; draining before accepting more actions"
            );
            tokio::select! {
                _ = shutdown.notified() => break,
                _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
            }
        }
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
            _ = directive_state_changed.notified() => continue,
        };

        let maybe_delivery = tokio::select! {
            _ = shutdown.notified() => {
                drop(permit);
                info!("worker loop shutting down");
                break;
            }
            _ = directive_state_changed.notified() => {
                drop(permit);
                continue;
            }
            result = broker.receive_for(&profile) => {
                match result {
                    Ok(delivery) => delivery,
                    Err(err @ BrokerError::Unauthorized(_)) => {
                        drop(permit);
                        loop_error = Some(broker_error("receive", err));
                        break;
                    }
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
        let in_flight = Arc::clone(&in_flight);
        let executor_leases = executor_leases.clone();
        let events = Arc::clone(&events);
        let result_outbox = Arc::clone(&result_outbox);
        let function_cache = Arc::clone(&function_cache);
        deliveries.spawn(async move {
            let _permit = permit;
            if let Err(err) = process_delivery(
                &broker,
                &consumer_id,
                libraries,
                api_client,
                providers,
                maybe_delivery,
                in_flight,
                executor_leases,
                events,
                result_outbox,
                function_cache,
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

    directive_task.abort();
    if let Err(err) = directive_task.await
        && !err.is_cancelled()
    {
        error!("agent directive task join error: {err}");
    }

    outbox_task.abort();
    if let Err(err) = outbox_task.await
        && !err.is_cancelled()
    {
        error!("worker result outbox task join error: {}", err);
    }

    if let Some(err) = loop_error {
        return Err(err);
    }
    if restart_requested.load(Ordering::SeqCst) {
        return Err(Box::new(std::io::Error::other(
            "agent restart directive requested a reconnect",
        )));
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
                    Err(err @ BrokerError::Unauthorized(_)) => {
                        return Err(broker_error("receive_control", err));
                    }
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
    delivery: BrokerDelivery,
    in_flight: Arc<Mutex<HashMap<Uuid, InFlightAction>>>,
    executor_leases: ExecutorLeaseManager,
    events: Arc<dyn WorkerEventSink>,
    result_outbox: Arc<dyn ResultOutbox>,
    function_cache: Arc<FunctionCache>,
) -> Result<(), SendableError> {
    // link this execution span to the trace that dispatched the action (w3c context from the broker
    // message). a no-op when the dispatcher had otel off.
    runinator_utilities::telemetry::apply_trace_context(
        &tracing::Span::current(),
        &delivery.command.trace_context,
    );
    metrics::action_received();
    metrics::action_queue_wait(
        (chrono::Utc::now() - delivery.enqueued_at)
            .num_milliseconds()
            .max(0) as f64,
    );
    let command = delivery.command.clone();
    let action = command.action.clone();
    let token = CancellationToken::new();
    {
        // insert only when vacant: a concurrent duplicate delivery of a node run already executing
        // here must not replace (and on its way out, remove) the original's cancellation
        // registration, and must never execute alongside it.
        let mut guard = in_flight.lock().await;
        if guard.contains_key(&command.workflow_node_run_id) {
            drop(guard);
            info!(
                node_run_id = %command.workflow_node_run_id,
                "skipping duplicate delivery: node run already executing on this worker"
            );
            metrics::action_duplicate();
            events.handle(WorkerEvent::ActionSkippedDuplicate {
                node_run_id: command.workflow_node_run_id,
            });
            return broker
                .ack(consumer_id, delivery.delivery_id)
                .await
                .map_err(|err| broker_error("ack", err));
        }
        guard.insert(
            command.workflow_node_run_id,
            InFlightAction {
                workflow_run_id: command.workflow_run_id,
                token: token.clone(),
                canceled_by_control: Arc::new(AtomicBool::new(false)),
            },
        );
    }
    let sink = RunOutputSink::new(
        command.clone(),
        broker.clone(),
        result_outbox,
        Some(crate::artifact_upload::ArtifactUploader::new(
            api_client.clone(),
        )),
        tokio::runtime::Handle::current(),
    );
    // acquire the execution lease before anything observable runs. a redelivered or timeout-raced
    // duplicate of this node run loses the claim and is dropped here, so the action never executes
    // twice concurrently. this deadline is only the backstop for a holder that is still live but has
    // lost the action; the server also frees the lease as soon as the holding replica stops
    // heartbeating, which is what bounds failover after a worker crash.
    if executor_leases.held_elsewhere(&command).await {
        info!(
            node_run_id = %command.workflow_node_run_id,
            "skipping duplicate delivery: executor lease held elsewhere"
        );
        metrics::action_duplicate();
        metrics::lease_contention();
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
    // reserve the declared idempotency key before anything the outside world can observe. a key whose
    // execution already completed settles this delivery from the recorded result; one held by another
    // node run makes this delivery a duplicate.
    let idempotency_key = match crate::idempotency::open_gate(&api_client, &command).await {
        crate::idempotency::IdempotencyGate::Execute { key } => key,
        crate::idempotency::IdempotencyGate::Replay { result } => {
            return settle_from_idempotent_replay(
                broker,
                consumer_id,
                &executor_leases,
                &in_flight,
                &events,
                &sink,
                &command,
                &action,
                delivery.delivery_id,
                result,
            )
            .await;
        }
        crate::idempotency::IdempotencyGate::Duplicate => {
            metrics::action_duplicate();
            events.handle(WorkerEvent::ActionSkippedDuplicate {
                node_run_id: command.workflow_node_run_id,
            });
            in_flight.lock().await.remove(&command.workflow_node_run_id);
            executor_leases.release_for_redelivery(&command).await;
            return broker
                .ack(consumer_id, delivery.delivery_id)
                .await
                .map_err(|err| broker_error("ack", err));
        }
    };
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
            &executor_leases,
            &command,
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
                &executor_leases,
                &command,
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
            // the provider never ran, so free the reservation taken above rather than leaving the key
            // blocked until it ages out.
            if let Some(key) = idempotency_key.as_deref() {
                crate::idempotency::release(&api_client, key, command.workflow_node_run_id).await;
            }
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
                    &executor_leases,
                    &command,
                    delivery.delivery_id,
                )
                .await?;
                return Err(broker_error("publish_result", err));
            }
            broker
                .ack(consumer_id, delivery.delivery_id)
                .await
                .map_err(|err| broker_error("ack", err))?;
            executor_leases.release_after_settlement(&command).await;
            in_flight.lock().await.remove(&command.workflow_node_run_id);
            return Ok(());
        }
    };
    // an action carrying a function binding needs its published code on this machine before the
    // provider can mount it. staging happens after secret resolution and before execution, in the
    // same place and for the same reason: it is a prerequisite the provider must not have to fetch
    // for itself.
    let parameters =
        match stage_packaged_function(&function_cache, &api_client, &action, &command, parameters)
            .await
        {
            Ok(parameters) => parameters,
            Err(err) => {
                let message = format!("Failed to stage packaged function: {err}");
                error!(
                    node_run_id = %command.workflow_node_run_id,
                    error_code = error_code_or_unknown(err.as_ref()),
                    "{}",
                    message
                );
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
                // the provider never ran, so free the reservation rather than leaving the key blocked.
                if let Some(key) = idempotency_key.as_deref() {
                    crate::idempotency::release(&api_client, key, command.workflow_node_run_id)
                        .await;
                }
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
                        "failed to publish failed status: {}",
                        err
                    );
                    in_flight.lock().await.remove(&command.workflow_node_run_id);
                    nack_action_delivery(
                        broker,
                        consumer_id,
                        &executor_leases,
                        &command,
                        delivery.delivery_id,
                    )
                    .await?;
                    return Err(broker_error("publish_result", err));
                }
                broker
                    .ack(consumer_id, delivery.delivery_id)
                    .await
                    .map_err(|err| broker_error("ack", err))?;
                executor_leases.release_after_settlement(&command).await;
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
            idempotency_key.clone(),
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
            &executor_leases,
            &command,
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
            &executor_leases,
            &command,
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
                &executor_leases,
                &command,
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
        // record against the reserved key *before* publishing. that ordering is the whole point: if
        // the publish below fails and the delivery is nacked, the redelivery replays this result
        // instead of re-running the side effect (appendix A.7).
        if let Some(key) = idempotency_key.as_deref() {
            crate::idempotency::record_success(
                &api_client,
                key,
                command.workflow_node_run_id,
                &runinator_models::orchestration::IdempotentActionResult {
                    success: true,
                    output_json: Some(output_json.clone()),
                    message: provider_message.clone(),
                },
            )
            .await;
        }
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
                &executor_leases,
                &command,
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
                &executor_leases,
                &command,
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
        // a failed attempt records nothing and frees the reservation: the node's own `.retry()` must
        // be able to run again, and replaying a failure forever would be the opposite of the point.
        if let Some(key) = idempotency_key.as_deref() {
            crate::idempotency::release(&api_client, key, command.workflow_node_run_id).await;
        }
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
                &executor_leases,
                &command,
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
    // side effects. a failed release is remembered so the retry the reducer schedules next can
    // reclaim the leftover lease here immediately, and still self-heals via staleness elsewhere.
    executor_leases.release_after_settlement(&command).await;
    Ok(())
}

/// return a delivery to the broker for redelivery, releasing this worker's executor lease first.
/// without the release the retry is lost: the executor claim is not re-entrant, so a redelivery
/// landing on another worker is dropped as a duplicate and acked until the lease goes stale,
/// parking the node run until the reducer's timeout backstop fires. a failed release is remembered
/// so a redelivery landing back on this worker reclaims the leftover lease instead.
async fn nack_action_delivery(
    broker: &Arc<dyn Broker>,
    consumer_id: &str,
    executor_leases: &ExecutorLeaseManager,
    command: &ActionCommand,
    delivery_id: uuid::Uuid,
) -> Result<(), SendableError> {
    executor_leases.release_for_redelivery(command).await;
    broker
        .nack(consumer_id, delivery_id)
        .await
        .map_err(|err| broker_error("nack", err))
}

/// settle a delivery from a result already recorded under its idempotency key, without invoking the
/// provider. the node run reaches the same terminal status the original execution reached, which is
/// what makes a redelivery after a failed publish harmless rather than a second side effect.
#[allow(clippy::too_many_arguments)]
async fn settle_from_idempotent_replay(
    broker: &Arc<dyn Broker>,
    consumer_id: &str,
    executor_leases: &ExecutorLeaseManager,
    in_flight: &Arc<Mutex<HashMap<Uuid, InFlightAction>>>,
    events: &Arc<dyn WorkerEventSink>,
    sink: &RunOutputSink,
    command: &ActionCommand,
    action: &WorkflowAction,
    delivery_id: Uuid,
    result: runinator_models::orchestration::IdempotentActionResult,
) -> Result<(), SendableError> {
    metrics::action_replayed();
    let status = match result.success {
        true => WorkflowStatus::Succeeded,
        false => WorkflowStatus::Failed,
    };
    let output_json = match result.output_json.clone() {
        Some(output) => output,
        None => TaskStatusOutput {
            success: result.success,
            duration_ms: Some(0),
            message: result.message.clone(),
        }
        .to_wire_value()?,
    };
    sink.emit_log(format!(
        "Action {}.{} skipped: replaying the result already recorded for this idempotency key.",
        action.provider, action.function
    ));
    if let Err(err) = sink.flush().await {
        in_flight.lock().await.remove(&command.workflow_node_run_id);
        nack_action_delivery(broker, consumer_id, executor_leases, command, delivery_id).await?;
        return Err(broker_error("publish_result", err));
    }
    if let Err(err) = sink
        .publish_status(status, Some(output_json), result.message.clone())
        .await
    {
        error!(
            node_run_id = %command.workflow_node_run_id,
            error_code = error_code_or_unknown(&err),
            "failed to publish replayed status: {}",
            err
        );
        in_flight.lock().await.remove(&command.workflow_node_run_id);
        nack_action_delivery(broker, consumer_id, executor_leases, command, delivery_id).await?;
        return Err(broker_error("publish_result", err));
    }
    events.handle(WorkerEvent::ActionFinished {
        workflow_run_id: command.workflow_run_id,
        node_id: command.node_id.clone(),
        node_run_id: command.workflow_node_run_id,
        provider: action.provider.clone(),
        function: action.function.clone(),
        outcome: match result.success {
            true => ActionOutcome::Succeeded,
            false => ActionOutcome::Failed,
        },
        duration_ms: 0,
        message: result.message.clone(),
    });
    broker
        .ack(consumer_id, delivery_id)
        .await
        .map_err(|err| broker_error("ack", err))?;
    executor_leases.release_after_settlement(command).await;
    in_flight.lock().await.remove(&command.workflow_node_run_id);
    Ok(())
}

/// stage a packaged function's code and rewrite the parameters into what the provider reads.
///
/// a plain action passes straight through: the binding is what marks a dispatch as packaged code,
/// and an action without one must be untouched by any of this.
async fn stage_packaged_function(
    cache: &Arc<FunctionCache>,
    api_client: &AsyncApiClient<StaticLocator>,
    action: &runinator_models::workflows::WorkflowAction,
    command: &runinator_comm::ActionCommand,
    parameters: runinator_models::value::Value,
) -> Result<runinator_models::value::Value, SendableError> {
    let Some(binding) = action.function_binding.as_ref() else {
        return Ok(parameters);
    };
    // the authored arguments arrive under `input`; everything else on the action is ours.
    let authored = parameters
        .get(crate::function_cache::INPUT_KEY)
        .cloned()
        .unwrap_or(parameters.clone());
    let context = runinator_models::json!({
        "package": binding.package_name.clone(),
        "export": binding.export_name.clone(),
        "version": binding.version,
        "workflow_run_id": command.workflow_run_id,
        "workflow_node_run_id": command.workflow_node_run_id,
        "attempt": command.attempt,
    });
    crate::function_cache::prepare_invocation(cache, api_client, binding, authored, context).await
}
