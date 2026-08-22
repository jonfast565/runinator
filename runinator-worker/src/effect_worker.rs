//! Provider execution for the workflow VM's generic effect protocol.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_broker::{Broker, BrokerError, EffectDelivery, EffectResultMessage};
use runinator_comm::{ConsumerProfile, EffectResult, EffectResultKind};
use runinator_models::{
    errors::{SendableError, error_code_or_unknown},
    orchestration::{IdempotencyClaim, IdempotentActionResult},
    runs::{NewRunArtifact, RunStatus},
    value::Value,
    workflow_vm::{WorkflowEffectRequest, WorkflowEffectStatus},
    workflows::{WorkflowAction, WorkflowObject},
};
use runinator_plugin::{cancel::CancellationToken, plugin::Plugin, provider::ProviderEventSink};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::{
    sync::{Mutex, Notify},
    task::JoinSet,
};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    executor,
    function_cache::{FunctionCache, INPUT_KEY, prepare_invocation},
    provider_repository::ProviderFactory,
    secrets::{is_transient_secret_error, resolve_secret_refs},
};

const RECEIVE_RETRY_BACKOFF: Duration = Duration::from_secs(1);
const SECRET_RETRY_BACKOFF: Duration = Duration::from_secs(5);

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_provider_effect_loop(
    broker: Arc<dyn Broker>,
    profile: ConsumerProfile,
    libraries: Arc<HashMap<String, Plugin>>,
    api_client: AsyncApiClient<StaticLocator>,
    providers: ProviderFactory,
    max_concurrent_effects: usize,
    shutdown_grace: Duration,
    in_flight: Arc<Mutex<HashMap<Uuid, crate::worker::InFlightAction>>>,
    result_outbox: Arc<dyn crate::agent::outbox::ResultOutbox>,
    shutdown: Arc<Notify>,
    events: Arc<dyn crate::events::WorkerEventSink>,
    drained: Arc<AtomicBool>,
) -> Result<(), SendableError> {
    let consumer = profile.id.clone();
    let executor_replica_id = profile.replica_id;
    let permits = Arc::new(tokio::sync::Semaphore::new(max_concurrent_effects.max(1)));
    let cache = Arc::new(FunctionCache::new(api_client.clone()));
    let mut tasks = JoinSet::new();
    info!("worker VM provider-effect loop started");

    loop {
        if drained.load(Ordering::Acquire) {
            tokio::select! {
                _ = shutdown.notified() => break,
                _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
            }
        }
        let permit = tokio::select! {
            _ = shutdown.notified() => break,
            Some(result) = tasks.join_next(), if !tasks.is_empty() => {
                if let Err(error) = result {
                    error!(%error, "provider-effect task join failed");
                }
                continue;
            }
            permit = permits.clone().acquire_owned() => permit
                .map_err(|error| crate::errors::CONCURRENCY_CLOSED.error(error))?,
        };
        let delivery = tokio::select! {
            _ = shutdown.notified() => {
                drop(permit);
                break;
            }
            result = broker.receive_effect_for(&profile) => match result {
                Ok(delivery) => delivery,
                Err(error @ BrokerError::Unauthorized(_)) => {
                    return Err(crate::broker::broker_error("receive_effect", error));
                }
                Err(error) => {
                    drop(permit);
                    error!(error_code = error_code_or_unknown(&error), %error, "failed to receive provider effect");
                    tokio::select! {
                        _ = shutdown.notified() => break,
                        _ = tokio::time::sleep(RECEIVE_RETRY_BACKOFF) => continue,
                    }
                }
            }
        };
        let broker = broker.clone();
        let consumer = consumer.clone();
        let libraries = libraries.clone();
        let api_client = api_client.clone();
        let providers = providers.clone();
        let cache = cache.clone();
        let in_flight = in_flight.clone();
        let result_outbox = result_outbox.clone();
        let events = events.clone();
        tasks.spawn(async move {
            let _permit = permit;
            if let Err(error) = process_provider_effect(
                broker,
                &consumer,
                executor_replica_id,
                libraries,
                api_client,
                providers,
                cache,
                in_flight,
                result_outbox,
                events,
                delivery,
            )
            .await
            {
                error!(error_code = error_code_or_unknown(error.as_ref()), %error, "provider effect failed");
            }
        });
    }
    let drain = async {
        while let Some(result) = tasks.join_next().await {
            if let Err(error) = result {
                error!(%error, "provider-effect task join failed during shutdown");
            }
        }
    };
    if tokio::time::timeout(shutdown_grace, drain).await.is_err() {
        warn!(
            shutdown_grace_secs = shutdown_grace.as_secs(),
            "provider effects exceeded shutdown grace; aborting them"
        );
        tasks.abort_all();
        while let Some(result) = tasks.join_next().await {
            if let Err(error) = result
                && !error.is_cancelled()
            {
                error!(%error, "provider-effect task join failed after abort");
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn process_provider_effect(
    broker: Arc<dyn Broker>,
    consumer: &str,
    executor_replica_id: Option<Uuid>,
    libraries: Arc<HashMap<String, Plugin>>,
    api_client: AsyncApiClient<StaticLocator>,
    providers: ProviderFactory,
    cache: Arc<FunctionCache>,
    in_flight: Arc<Mutex<HashMap<Uuid, crate::worker::InFlightAction>>>,
    result_outbox: Arc<dyn crate::agent::outbox::ResultOutbox>,
    events: Arc<dyn crate::events::WorkerEventSink>,
    delivery: EffectDelivery,
) -> Result<(), SendableError> {
    let command = delivery.command;
    command
        .ensure_supported()
        .map_err(|error| -> SendableError { Box::new(error) })?;
    runinator_utilities::telemetry::apply_trace_context(
        &tracing::Span::current(),
        &command.trace_context,
    );
    let WorkflowEffectRequest::Action {
        provider,
        function,
        input,
        timeout_seconds,
        tags,
        required_labels,
        idempotency_key,
        function_binding,
        ..
    } = command.request.clone()
    else {
        publish_terminal(
            broker.as_ref(),
            result_outbox.as_ref(),
            &command,
            WorkflowEffectStatus::Failed,
            None,
            Some("provider worker received a non-provider effect".into()),
        )
        .await?;
        broker
            .ack_effect(consumer, delivery.delivery_id)
            .await
            .map_err(|error| crate::broker::broker_error("ack_effect", error))?;
        return Ok(());
    };

    let input = match resolve_secret_refs(&api_client, input).await {
        Ok(input) => input,
        Err(error) if is_transient_secret_error(&error) => {
            warn!(effect_id = %command.effect_id, %error, "returning provider effect after transient secret resolution failure");
            tokio::time::sleep(SECRET_RETRY_BACKOFF).await;
            broker
                .nack_effect(consumer, delivery.delivery_id)
                .await
                .map_err(|error| crate::broker::broker_error("nack_effect", error))?;
            return Ok(());
        }
        Err(error) => {
            crate::metrics::secret_resolution_failure();
            publish_terminal(
                broker.as_ref(),
                result_outbox.as_ref(),
                &command,
                WorkflowEffectStatus::Failed,
                None,
                Some(format!("failed to resolve action secrets: {error}")),
            )
            .await?;
            broker
                .ack_effect(consumer, delivery.delivery_id)
                .await
                .map_err(|error| crate::broker::broker_error("ack_effect", error))?;
            return Ok(());
        }
    };
    let input = if let Some(binding) = function_binding.as_ref() {
        let authored = input.get(INPUT_KEY).cloned().unwrap_or(input);
        prepare_invocation(
            &cache,
            &api_client,
            binding,
            authored,
            runinator_models::json!({
                "package": binding.package_name,
                "export": binding.export_name,
                "version": binding.version,
                "workflow_run_id": command.workflow_run_id,
                "effect_id": command.effect_id,
                "attempt": command.attempt,
            }),
        )
        .await?
    } else {
        input
    };
    let configuration = WorkflowObject::from_value(input.clone()).map_err(|message| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            message,
        )) as SendableError
    })?;
    let action = WorkflowAction {
        provider,
        function,
        timeout_seconds: timeout_seconds
            .unwrap_or(runinator_models::workflow_vm::DEFAULT_ACTION_TIMEOUT_SECONDS),
        configuration,
        mcp_enabled: false,
        tags,
        required_labels,
        idempotency_key: idempotency_key.clone(),
        function_binding,
    };
    let provider_name = action.provider.clone();
    let function_name = action.function.clone();
    match api_client
        .claim_idempotency_key(
            &command.idempotency_key,
            command.effect_id,
            action.timeout_seconds,
        )
        .await
    {
        Ok(IdempotencyClaim::Completed { result }) => {
            let recorded = result.decode::<IdempotentActionResult>()?;
            publish_terminal(
                broker.as_ref(),
                result_outbox.as_ref(),
                &command,
                if recorded.success {
                    WorkflowEffectStatus::Succeeded
                } else {
                    WorkflowEffectStatus::Failed
                },
                recorded.output_json,
                recorded.message,
            )
            .await?;
            broker
                .ack_effect(consumer, delivery.delivery_id)
                .await
                .map_err(|error| crate::broker::broker_error("ack_effect", error))?;
            return Ok(());
        }
        Ok(IdempotencyClaim::Held { owner_node_run_id }) => {
            warn!(effect_id = %command.effect_id, owner = %owner_node_run_id, "provider effect idempotency key is held elsewhere; returning delivery");
            broker
                .nack_effect(consumer, delivery.delivery_id)
                .await
                .map_err(|error| crate::broker::broker_error("nack_effect", error))?;
            return Ok(());
        }
        Ok(IdempotencyClaim::Acquired) => {}
        Err(error) => {
            // Provider-native idempotency still receives the frozen key below. The reservation is
            // an additional crash-window guard, so an API outage must not make the worker deadlock.
            warn!(effect_id = %command.effect_id, %error, "effect idempotency reservation unavailable; executing with provider key only");
        }
    }
    let provider_key = idempotency_key.as_ref().map(value_key);
    crate::metrics::effect_received();
    let token = CancellationToken::new();
    let canceled_by_control = Arc::new(AtomicBool::new(false));
    {
        let mut guard = in_flight.lock().await;
        if guard.contains_key(&command.effect_id) {
            broker
                .ack_effect(consumer, delivery.delivery_id)
                .await
                .map_err(|error| crate::broker::broker_error("ack_effect", error))?;
            return Ok(());
        }
        guard.insert(
            command.effect_id,
            crate::worker::InFlightAction {
                workflow_run_id: command.workflow_run_id,
                token: token.clone(),
                canceled_by_control: canceled_by_control.clone(),
            },
        );
    }
    events.handle(crate::events::WorkerEvent::EffectStarted {
        workflow_run_id: command.workflow_run_id,
        effect_id: command.effect_id,
        provider: provider_name.clone(),
        function: function_name.clone(),
        attempt: i64::from(command.attempt),
    });
    // take the effect's executor lease before running it. this is best-effort and deliberately not
    // durable: it is what the replica views and the stale-replica reaper read, and losing it must
    // never stop the effect from executing or settling.
    if let Some(replica_id) = executor_replica_id {
        let mut claim = EffectResult::claimed(&command, replica_id);
        claim.event_id = stable_event_id(command.effect_id, "claim");
        if let Err(error) =
            publish_result(broker.as_ref(), result_outbox.as_ref(), &mut claim, false).await
        {
            warn!(effect_id = %command.effect_id, %error, "failed to publish effect executor claim");
        }
    }
    let _in_flight_metric = crate::metrics::in_flight_guard();
    let output_sink = Arc::new(EffectOutputSink::new(
        command.clone(),
        broker.clone(),
        api_client.clone(),
        result_outbox.clone(),
    ));
    let outcome = executor::execute_task(
        &providers,
        libraries,
        action,
        command.effect_id,
        input,
        provider_key,
        Some(output_sink.clone()),
        token.clone(),
    )
    .await;

    in_flight.lock().await.remove(&command.effect_id);

    if token.is_cancelled() && !canceled_by_control.load(Ordering::Acquire) {
        broker
            .nack_effect(consumer, delivery.delivery_id)
            .await
            .map_err(|error| crate::broker::broker_error("nack_effect", error))?;
        return Ok(());
    }

    output_sink.flush().await?;

    if let Some(result) = &outcome.execution_result {
        for (index, artifact) in result.artifacts.iter().enumerate() {
            let mut artifact = artifact.clone();
            output_sink
                .uploader
                .relocate_effect(&command, &mut artifact)
                .await;
            let mut event = EffectResult {
                version: command.version,
                event_id: stable_event_id(command.effect_id, &format!("artifact:{index}")),
                effect_id: command.effect_id,
                workflow_run_id: command.workflow_run_id,
                continuation_id: command.continuation_id,
                attempt: command.attempt,
                kind: EffectResultKind::Artifact {
                    artifact: Value::encode(&artifact)?,
                },
                timestamp: chrono::Utc::now(),
                trace_id: command.trace_id,
                notification_delivery_id: command.notification_delivery_id,
            };
            publish_result(broker.as_ref(), result_outbox.as_ref(), &mut event, true).await?;
        }
    }
    let status = match outcome.status {
        RunStatus::Succeeded => WorkflowEffectStatus::Succeeded,
        RunStatus::TimedOut => WorkflowEffectStatus::TimedOut,
        RunStatus::Canceled => WorkflowEffectStatus::Canceled,
        _ => WorkflowEffectStatus::Failed,
    };
    let event_outcome = match status {
        WorkflowEffectStatus::Succeeded => crate::events::ActionOutcome::Succeeded,
        WorkflowEffectStatus::TimedOut => crate::events::ActionOutcome::TimedOut,
        WorkflowEffectStatus::Canceled => crate::events::ActionOutcome::Canceled,
        _ => crate::events::ActionOutcome::Failed,
    };
    let output = outcome
        .execution_result
        .as_ref()
        .and_then(|result| result.output_json.clone());
    if status == WorkflowEffectStatus::Succeeded {
        let recorded = IdempotentActionResult {
            success: true,
            output_json: output.clone(),
            message: outcome.task_result.message.clone(),
        };
        if let Ok(value) = Value::encode(&recorded) {
            let _ = api_client
                .complete_idempotency_key(&command.idempotency_key, command.effect_id, value)
                .await;
        }
    } else {
        let _ = api_client
            .release_idempotency_key(&command.idempotency_key, command.effect_id)
            .await;
    }
    publish_terminal(
        broker.as_ref(),
        result_outbox.as_ref(),
        &command,
        status,
        output,
        outcome.task_result.message.clone(),
    )
    .await?;
    events.handle(crate::events::WorkerEvent::EffectFinished {
        workflow_run_id: command.workflow_run_id,
        effect_id: command.effect_id,
        provider: provider_name,
        function: function_name,
        outcome: event_outcome,
        duration_ms: outcome.task_result.duration_ms(),
        message: outcome.task_result.message.clone(),
    });
    crate::metrics::effect_completed(
        event_outcome.as_str(),
        outcome.task_result.duration_ms() as f64,
    );
    broker
        .ack_effect(consumer, delivery.delivery_id)
        .await
        .map_err(|error| crate::broker::broker_error("ack_effect", error))?;
    Ok(())
}

fn value_key(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        value => serde_json::to_string(value).unwrap_or_else(|_| value.to_string()),
    }
}

async fn publish_terminal(
    broker: &dyn Broker,
    outbox: &dyn crate::agent::outbox::ResultOutbox,
    command: &runinator_comm::EffectCommand,
    status: WorkflowEffectStatus,
    output: Option<Value>,
    message: Option<String>,
) -> Result<(), SendableError> {
    let mut result = EffectResult::status(command, status, output, message);
    result.event_id = stable_event_id(command.effect_id, "terminal");
    publish_result(broker, outbox, &mut result, true).await
}

async fn publish_result(
    broker: &dyn Broker,
    outbox: &dyn crate::agent::outbox::ResultOutbox,
    result: &mut EffectResult,
    durable: bool,
) -> Result<(), SendableError> {
    let event_id = result.event_id;
    let message = EffectResultMessage {
        result: result.clone(),
        dedupe_key: Some(event_id.to_string()),
        enqueued_at: chrono::Utc::now(),
    };
    match broker.publish_effect_result(message.clone()).await {
        Ok(()) | Err(BrokerError::Duplicate(_)) => Ok(()),
        Err(error) if durable => outbox.append_effect(message).map_err(
            |outbox_error| -> SendableError {
                Box::new(std::io::Error::other(format!(
                    "effect result publish failed ({error}); durable outbox failed: {outbox_error}"
                )))
            },
        ),
        // Chunks are intentionally best effort and never cause a provider action to execute again.
        Err(error) => {
            warn!(%error, effect_id = %result.effect_id, "dropping workflow effect chunk after broker publish failure");
            Ok(())
        }
    }
}

fn stable_event_id(effect_id: Uuid, boundary: &str) -> Uuid {
    Uuid::new_v5(&effect_id, boundary.as_bytes())
}

struct EffectOutputSink {
    command: runinator_comm::EffectCommand,
    broker: Arc<dyn Broker>,
    uploader: Arc<crate::artifact_upload::ArtifactUploader>,
    outbox: Arc<dyn crate::agent::outbox::ResultOutbox>,
    handle: tokio::runtime::Handle,
    pending: StdMutex<Vec<tokio::task::JoinHandle<Result<(), SendableError>>>>,
}

impl EffectOutputSink {
    fn new(
        command: runinator_comm::EffectCommand,
        broker: Arc<dyn Broker>,
        api_client: AsyncApiClient<StaticLocator>,
        outbox: Arc<dyn crate::agent::outbox::ResultOutbox>,
    ) -> Self {
        Self {
            command,
            broker,
            uploader: crate::artifact_upload::ArtifactUploader::new(api_client),
            outbox,
            handle: tokio::runtime::Handle::current(),
            pending: StdMutex::new(Vec::new()),
        }
    }

    fn spawn(
        &self,
        task: impl std::future::Future<Output = Result<(), SendableError>> + Send + 'static,
    ) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.push(self.handle.spawn(task));
        }
    }

    async fn flush(&self) -> Result<(), SendableError> {
        let pending = self
            .pending
            .lock()
            .map(|mut pending| std::mem::take(&mut *pending))
            .unwrap_or_default();
        for task in pending {
            task.await
                .map_err(|error| -> SendableError { Box::new(error) })??;
        }
        Ok(())
    }
}

impl ProviderEventSink for EffectOutputSink {
    fn emit(&self, event: runinator_models::runs::ProviderExecutionEvent) {
        match event {
            runinator_models::runs::ProviderExecutionEvent::Chunk { stream, content } => {
                let broker = self.broker.clone();
                let command = self.command.clone();
                let outbox = self.outbox.clone();
                self.spawn(async move {
                    let mut result = EffectResult {
                        version: command.version,
                        event_id: Uuid::now_v7(),
                        effect_id: command.effect_id,
                        workflow_run_id: command.workflow_run_id,
                        continuation_id: command.continuation_id,
                        attempt: command.attempt,
                        kind: EffectResultKind::Chunk { stream, content },
                        timestamp: chrono::Utc::now(),
                        trace_id: command.trace_id,
                        notification_delivery_id: command.notification_delivery_id,
                    };
                    publish_result(broker.as_ref(), outbox.as_ref(), &mut result, false).await
                });
            }
            runinator_models::runs::ProviderExecutionEvent::Artifact {
                name,
                mime_type,
                size_bytes,
                uri,
                metadata,
            } => {
                let broker = self.broker.clone();
                let command = self.command.clone();
                let uploader = self.uploader.clone();
                let outbox = self.outbox.clone();
                self.spawn(async move {
                    let mut artifact = NewRunArtifact {
                        name,
                        mime_type,
                        size_bytes,
                        uri,
                        metadata,
                    };
                    uploader.relocate_effect(&command, &mut artifact).await;
                    let mut result = EffectResult {
                        version: command.version,
                        event_id: Uuid::now_v7(),
                        effect_id: command.effect_id,
                        workflow_run_id: command.workflow_run_id,
                        continuation_id: command.continuation_id,
                        attempt: command.attempt,
                        kind: EffectResultKind::Artifact {
                            artifact: Value::encode(&artifact)?,
                        },
                        timestamp: chrono::Utc::now(),
                        trace_id: command.trace_id,
                        notification_delivery_id: command.notification_delivery_id,
                    };
                    publish_result(broker.as_ref(), outbox.as_ref(), &mut result, true).await
                });
            }
            runinator_models::runs::ProviderExecutionEvent::Message { .. } => {}
        }
    }
}
