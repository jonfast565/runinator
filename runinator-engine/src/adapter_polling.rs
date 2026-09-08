//! Durable GitHub/Jira adapter polling. The loop claims persisted schedules and feeds normalized
//! events through the same pipeline-ingress service used by webhook HTTP handlers.

use std::{sync::Arc, time::Duration};

use chrono::{TimeDelta, Utc};
use runinator_adapter_contract::{AdapterPollRequest, AdapterPollResponse};
use runinator_broker_core::{ActionTarget, Broker, EmbeddedEngineSignals};
use runinator_comm::{EffectCommand, EffectExecutor};
use runinator_models::{
    auth::ResourceType,
    orchestration::{AdapterAuthentication, AdapterTransport},
    value::Value,
    workflow_vm::{WORKFLOW_EFFECT_PROTOCOL_VERSION, WorkflowEffectRequest},
    workflows::WorkflowRetry,
};
use runinator_store::roles::AdapterPollDispatch;
use tokio::sync::Notify;
use tracing::{debug, error, warn};

use crate::{
    engine::BackgroundEngineStore,
    events::EventSender,
    services::{AdapterOperations, PipelineOperations},
};

/// A claim must outlive the worst case for the poll it covers: the adapter-host request budget
/// plus ingesting the batch it returns. A claim is taken immediately before its poll starts, so
/// this is measured from the right instant rather than from the head of a batch.
const LEASE_SECONDS: i64 = 300;

/// How many adapters one pass will service before returning to the top of the loop. This bounds
/// how long a shutdown waits, not how many adapters can exist.
const MAX_ADAPTERS_PER_PASS: usize = 16;

const DEFAULT_RETRY_SECONDS: i64 = 60;
const MIN_INTERVAL_SECONDS: i64 = 30;
const MAX_INTERVAL_SECONDS: i64 = 3_600;

struct PollFailure {
    message: String,
    retry_after_seconds: i64,
}

impl From<String> for PollFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            retry_after_seconds: DEFAULT_RETRY_SECONDS,
        }
    }
}

impl From<runinator_adapter_client::AdapterClientError> for PollFailure {
    fn from(error: runinator_adapter_client::AdapterClientError) -> Self {
        match error {
            runinator_adapter_client::AdapterClientError::CircuitOpen {
                retry_after_seconds,
            } => Self {
                message: "adapter-host circuit is open".into(),
                retry_after_seconds: i64::try_from(retry_after_seconds)
                    .unwrap_or(MAX_INTERVAL_SECONDS)
                    .clamp(MIN_INTERVAL_SECONDS, MAX_INTERVAL_SECONDS),
            },
            error => Self::from(error.to_string()),
        }
    }
}

fn interval_seconds(configuration: &runinator_models::value::Value) -> i64 {
    configuration
        .get("poll_interval_seconds")
        .and_then(|value| value.as_i64())
        .unwrap_or(DEFAULT_RETRY_SECONDS)
        .clamp(MIN_INTERVAL_SECONDS, MAX_INTERVAL_SECONDS)
}

#[derive(Default)]
struct BatchSummary {
    accepted: usize,
    skipped: usize,
}

async fn poll_one<T: BackgroundEngineStore>(
    store: Arc<T>,
    _broker: &Arc<dyn Broker>,
    pipelines: &PipelineOperations<T>,
    instance: &str,
    status: runinator_models::orchestration::AdapterPollStatus,
) -> Result<BatchSummary, PollFailure> {
    let adapters = AdapterOperations::new(store.clone());
    let adapter = adapters
        .fetch(status.adapter_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "polling adapter no longer exists".to_string())?;
    let revision = adapters
        .current_revision(&adapter)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "polling adapter current revision is missing".to_string())?;
    if !adapter.enabled
        || revision.transport != AdapterTransport::Polling
        || revision.revision != status.revision
    {
        return Err("poll claim no longer matches an enabled polling revision"
            .to_string()
            .into());
    }
    let poll_request = AdapterPollRequest {
        configuration: serde_json::to_value(revision.configuration.clone()).unwrap_or_default(),
        secrets: serde_json::Value::Null,
        checkpoint: serde_json::to_value(status.checkpoint.clone()).unwrap_or_default(),
        initialize: status.checkpoint.is_null(),
    };
    let attempt_id = create_poll_attempt(
        store.clone(),
        PollAttemptRequest {
            adapter: &adapter,
            revision: &revision,
            request: poll_request,
            claim_owner: instance.into(),
            dry_run: false,
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    if matches!(
        revision.authentication,
        AdapterAuthentication::ExecutionProfile { .. }
    ) {
        return Ok(BatchSummary::default());
    }
    let secret_bindings = revision
        .authentication
        .secret_bindings()
        .cloned()
        .unwrap_or_default();
    for setting_id in secret_bindings.values().copied() {
        if !runinator_store::resource_access::resource_can_consume(
            store.as_ref(),
            ResourceType::OrchestrationAdapter,
            adapter.id,
            ResourceType::Setting,
            setting_id,
        )
        .await
        .map_err(|error| error.to_string())?
        {
            return Err(format!(
                "adapter {} is not permitted to use setting {setting_id}",
                adapter.id
            )
            .into());
        }
    }
    let secrets = adapters
        .resolve_secrets(adapter.org_id, &secret_bindings)
        .await?;
    let initialize = status.checkpoint.is_null();
    let response = runinator_adapter_client::poll(
        &adapter.kind,
        AdapterPollRequest {
            configuration: serde_json::to_value(revision.configuration.clone()).unwrap_or_default(),
            secrets,
            checkpoint: serde_json::to_value(status.checkpoint.clone()).unwrap_or_default(),
            initialize,
        },
    )
    .await;
    let outcome = match response {
        Ok(response) => {
            finish_poll_response(
                store.clone(),
                pipelines,
                adapter,
                revision,
                instance,
                attempt_id,
                response,
            )
            .await
        }
        Err(error) => Err(error.into()),
    };
    let error = outcome
        .as_ref()
        .err()
        .map(|failure| failure.message.clone());
    store
        .finish_adapter_poll_attempt(
            attempt_id,
            if error.is_some() {
                "failed"
            } else {
                "succeeded"
            }
            .into(),
            Value::Null,
            error,
            Utc::now(),
        )
        .await
        .map_err(|error| error.to_string())?;
    outcome
}

pub(crate) async fn settle_dispatched_poll<T: BackgroundEngineStore>(
    store: Arc<T>,
    pipelines: &PipelineOperations<T>,
    dispatch: &AdapterPollDispatch,
    response: AdapterPollResponse,
) -> Result<(), String> {
    let adapters = AdapterOperations::new(store.clone());
    let adapter = adapters
        .fetch(dispatch.adapter_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "polling adapter no longer exists".to_string())?;
    let revision = adapters
        .current_revision(&adapter)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "polling adapter current revision is missing".to_string())?;
    if !adapter.enabled
        || revision.transport != AdapterTransport::Polling
        || revision.revision != dispatch.adapter_revision
    {
        return Err("poll dispatch no longer matches an enabled polling revision".into());
    }
    finish_poll_response(
        store,
        pipelines,
        adapter,
        revision,
        &dispatch.claim_owner,
        dispatch.id,
        response,
    )
    .await
    .map(|_| ())
    .map_err(|failure| failure.message)
}

async fn finish_poll_response<T: BackgroundEngineStore>(
    store: Arc<T>,
    _pipelines: &PipelineOperations<T>,
    adapter: runinator_models::orchestration::AdapterDefinition,
    revision: runinator_models::orchestration::AdapterRevision,
    instance: &str,
    attempt_id: uuid::Uuid,
    response: AdapterPollResponse,
) -> Result<BatchSummary, PollFailure> {
    let adapters = AdapterOperations::new(store.clone());
    if let Some(error) = response.error {
        return Err(PollFailure {
            message: error,
            retry_after_seconds: response
                .retry_after_seconds
                .and_then(|value| i64::try_from(value).ok())
                .unwrap_or(DEFAULT_RETRY_SECONDS)
                .clamp(MIN_INTERVAL_SECONDS, MAX_INTERVAL_SECONDS),
        });
    }
    let mut summary = BatchSummary::default();
    for event in response.events {
        adapters
            .capture_delivery(
                runinator_models::adapter_control::AdapterOrigin {
                    adapter_id: adapter.id,
                    revision: revision.revision,
                    delivery_record_id: None,
                },
                Some(attempt_id),
                Some(event),
                None,
            )
            .await
            .map_err(|error| error.to_string())?;
        summary.accepted += 1;
    }
    let next = Utc::now() + TimeDelta::seconds(interval_seconds(&revision.configuration));
    let committed = store
        .complete_orchestration_adapter_poll(
            adapter.id,
            instance.into(),
            revision.revision,
            response.checkpoint.into(),
            next,
            Utc::now(),
        )
        .await
        .map_err(|error| error.to_string())?;
    if !committed {
        // the claim was stolen mid-poll, so this checkpoint is discarded and the batch will be
        // enumerated again by whoever holds the lease now. silence here is what made an expired
        // lease look identical to a successful poll.
        warn!(
            adapter_id = %adapter.id,
            "adapter poll finished without its claim; checkpoint was not advanced"
        );
    }
    Ok(summary)
}

pub async fn run_adapter_poll_loop<T: BackgroundEngineStore>(
    store: Arc<T>,
    broker: Arc<dyn Broker>,
    events: EventSender,
    signals: EmbeddedEngineSignals,
    _instance: String,
    shutdown: Arc<Notify>,
) {
    let pipelines = PipelineOperations::new(store.clone(), broker.clone(), events, Some(signals));
    loop {
        // one claim at a time: the lease has to start when the poll starts, not when the head of a
        // batch was claimed, or a slow adapter early in the batch expires the leases behind it.
        for _ in 0..MAX_ADAPTERS_PER_PASS {
            let now = Utc::now();
            let claim_owner = uuid::Uuid::now_v7().to_string();
            let claim = match store
                .claim_due_orchestration_adapter_polls(
                    claim_owner.clone(),
                    now,
                    now + TimeDelta::seconds(LEASE_SECONDS),
                    1,
                )
                .await
            {
                Ok(claims) => claims.into_iter().next(),
                Err(err) => {
                    error!("failed to claim due adapter polls: {err}");
                    break;
                }
            };
            let Some(claim) = claim else { break };
            match poll_one(
                store.clone(),
                &broker,
                &pipelines,
                &claim_owner,
                claim.clone(),
            )
            .await
            {
                Ok(summary) => debug!(
                    adapter_id = %claim.adapter_id,
                    "adapter poll accepted {} events and skipped {}",
                    summary.accepted, summary.skipped
                ),
                Err(failure) => {
                    warn!(adapter_id = %claim.adapter_id, "adapter poll failed: {}", failure.message);
                    let _ = store
                        .fail_orchestration_adapter_poll(
                            claim.adapter_id,
                            claim_owner.clone(),
                            Utc::now() + TimeDelta::seconds(failure.retry_after_seconds),
                            failure.message,
                            Utc::now(),
                        )
                        .await;
                }
            }
        }
        tokio::select! { _ = shutdown.notified() => return, _ = tokio::time::sleep(Duration::from_secs(1)) => {} }
    }
}

pub struct PollAttemptRequest<'a> {
    pub adapter: &'a runinator_models::orchestration::AdapterDefinition,
    pub revision: &'a runinator_models::orchestration::AdapterRevision,
    pub request: AdapterPollRequest,
    pub claim_owner: String,
    pub dry_run: bool,
}

pub async fn create_poll_attempt<
    T: runinator_store::roles::OrchestrationStore
        + runinator_store::roles::RbacStore
        + runinator_store::roles::AuthStore,
>(
    store: Arc<T>,
    input: PollAttemptRequest<'_>,
) -> Result<uuid::Uuid, runinator_models::errors::SendableError> {
    let PollAttemptRequest {
        adapter,
        revision,
        mut request,
        claim_owner,
        dry_run,
    } = input;
    let (profile, required_labels, required_scopes) = match &revision.authentication {
        AdapterAuthentication::ExecutionProfile {
            profile,
            required_labels,
            required_scopes,
        } => (
            Some(profile.clone()),
            required_labels.clone(),
            required_scopes.clone(),
        ),
        _ => (None, Default::default(), Vec::new()),
    };
    if let Some(profile) = &profile
        && !runinator_store::resource_access::resource_can_consume(
            store.as_ref(),
            ResourceType::OrchestrationAdapter,
            adapter.id,
            ResourceType::ExecutionProfile,
            profile.id(),
        )
        .await?
    {
        return Err(
            crate::errors::ADAPTER_EVENT_REJECTED.error("adapter cannot consume execution profile")
        );
    }
    request.secrets = serde_json::Value::Null;
    let id = uuid::Uuid::now_v7();
    let now = Utc::now();
    let state = if profile.is_some() {
        "queued"
    } else {
        "running"
    };
    let profile_id = profile
        .as_ref()
        .map(|profile| profile.id())
        .unwrap_or_else(uuid::Uuid::nil);
    let command = EffectCommand {
        version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
        command_id: uuid::Uuid::now_v7(),
        effect_id: id,
        workflow_run_id: adapter.id,
        continuation_id: uuid::Uuid::nil(),
        attempt: 1,
        request: WorkflowEffectRequest::Action {
            provider: "__runinator_adapter".into(),
            function: "poll".into(),
            input: Value::from(
                serde_json::json!({"kind":adapter.kind,"required_scopes":required_scopes,"request":request}),
            ),
            timeout_seconds: Some(120),
            retry: WorkflowRetry::default(),
            tags: Vec::new(),
            required_labels: required_labels.clone(),
            workspace_affinity: None,
            execution_profile: profile,
            idempotency_key: None,
            function_binding: None,
        },
        executor: EffectExecutor::Provider,
        target: ActionTarget::labels(required_labels),
        trace_id: id,
        trace_context: Default::default(),
        idempotency_key: format!("adapter-poll:{id}"),
        notification_delivery_id: None,
    };
    store
        .insert_orchestration_adapter_poll_dispatch(AdapterPollDispatch {
            id,
            adapter_id: adapter.id,
            adapter_revision: revision.revision,
            profile_id,
            claim_owner,
            command,
            state: state.into(),
            dry_run,
            deadline_at: now + TimeDelta::seconds(LEASE_SECONDS),
            created_at: now,
            updated_at: now,
        })
        .await?;
    Ok(id)
}
