//! Durable GitHub/Jira adapter polling. The loop claims persisted schedules and feeds normalized
//! events through the same pipeline-ingress service used by webhook HTTP handlers.

use std::{sync::Arc, time::Duration};

use chrono::{TimeDelta, Utc};
use runinator_adapter_contract::AdapterPollRequest;
use runinator_broker_core::{Broker, EmbeddedEngineSignals};
use runinator_models::orchestration::AdapterTransport;
use tokio::sync::Notify;
use tracing::{debug, error, warn};

use crate::{
    engine::BackgroundEngineStore,
    events::EventSender,
    services::{
        AdapterOperations, PipelineIngressError, PipelineIngressRequest, PipelineOperations,
    },
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

fn interval_seconds(configuration: &runinator_models::value::Value) -> i64 {
    configuration
        .get("poll_interval_seconds")
        .and_then(|value| value.as_i64())
        .unwrap_or(DEFAULT_RETRY_SECONDS)
        .clamp(MIN_INTERVAL_SECONDS, MAX_INTERVAL_SECONDS)
}

/// Whether a per-event outcome describes the event rather than the adapter.
///
/// A polling adapter enumerates everything upstream has, so a batch routinely contains events no
/// pipeline admits — an unrelated workflow run, an issue outside any admission. Those are ordinary
/// and must not fail the poll: failing would leave the checkpoint unadvanced, replay the identical
/// batch on the next tick, and stall the adapter permanently on its first unroutable event. Only a
/// fault that would recur for *every* event (the host being down, the store being unreachable)
/// justifies abandoning the batch.
fn is_event_scoped(error: &PipelineIngressError) -> bool {
    matches!(
        error,
        PipelineIngressError::NotFound(_)
            | PipelineIngressError::Invalid(_)
            | PipelineIngressError::Conflict(_)
            | PipelineIngressError::Held(_)
            | PipelineIngressError::Full
    )
}

#[derive(Default)]
struct BatchSummary {
    accepted: usize,
    skipped: usize,
}

async fn poll_one<T: BackgroundEngineStore>(
    store: Arc<T>,
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
    let secrets = adapters
        .resolve_secrets(adapter.org_id, &revision.secret_bindings)
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
    .await?;
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
    for mut event in response.events {
        if let Err(error) = event.validate_identity() {
            warn!(
                adapter_id = %adapter.id,
                "skipping polled event with an invalid identity: {error}"
            );
            summary.skipped += 1;
            continue;
        }
        if let Err(error) = adapters
            .resolve_correlation_alias(adapter.org_id, &mut event)
            .await
        {
            warn!(
                adapter_id = %adapter.id, delivery_id = %event.delivery_id,
                "skipping polled event whose correlation alias could not be resolved: {error}"
            );
            summary.skipped += 1;
            continue;
        }
        if let Some(payload) = event.payload.as_object_mut() {
            if let Some(subject_revision) = event.subject_revision.clone() {
                payload.insert("subject_revision".into(), subject_revision.into());
            }
            if !event.provenance.is_null() {
                payload.insert("provenance".into(), event.provenance.clone());
            }
        }
        let pipeline_id = match adapters.pipeline_for_event(&adapter, &event).await {
            Ok(value) => value,
            Err(error) => {
                debug!(
                    adapter_id = %adapter.id, delivery_id = %event.delivery_id,
                    "polled event matched no pipeline admission route: {error}"
                );
                summary.skipped += 1;
                continue;
            }
        };
        let outcome = pipelines
            .process_ingress(
                pipeline_id,
                Some(adapter.org_id),
                PipelineIngressRequest {
                    source: format!("adapter:{}:{}", adapter.id, event.source),
                    event_id: event.delivery_id.clone(),
                    event_type: event.event_type,
                    correlation_key: event.correlation_key,
                    payload: event.payload,
                    provenance: event.provenance,
                    occurred_at: event.occurred_at,
                },
                Some((adapter.id, revision.revision)),
            )
            .await;
        match outcome {
            Ok(_) => summary.accepted += 1,
            Err(error) if is_event_scoped(&error) => {
                debug!(
                    adapter_id = %adapter.id, delivery_id = %event.delivery_id,
                    "polled event was not admitted: {error:?}"
                );
                summary.skipped += 1;
            }
            Err(error) => return Err(format!("{error:?}").into()),
        }
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
    instance: String,
    shutdown: Arc<Notify>,
) {
    let pipelines = PipelineOperations::new(store.clone(), broker, events, Some(signals));
    loop {
        // one claim at a time: the lease has to start when the poll starts, not when the head of a
        // batch was claimed, or a slow adapter early in the batch expires the leases behind it.
        for _ in 0..MAX_ADAPTERS_PER_PASS {
            let now = Utc::now();
            let claim = match store
                .claim_due_orchestration_adapter_polls(
                    instance.clone(),
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
            match poll_one(store.clone(), &pipelines, &instance, claim.clone()).await {
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
                            instance.clone(),
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
