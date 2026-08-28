//! Durable GitHub/Jira adapter polling. The loop claims persisted schedules and feeds normalized
//! events through the same pipeline-ingress service used by webhook HTTP handlers.

use std::{sync::Arc, time::Duration};

use chrono::{TimeDelta, Utc};
use runinator_adapter_contract::{AdapterPollRequest, AdapterPollResponse};
use runinator_broker_core::{Broker, EmbeddedEngineSignals};
use runinator_models::orchestration::AdapterTransport;
use tokio::sync::Notify;
use tracing::{error, warn};

use crate::{
    engine::BackgroundEngineStore,
    events::EventSender,
    services::{AdapterOperations, PipelineIngressRequest, PipelineOperations},
};

struct PollFailure {
    message: String,
    retry_after_seconds: i64,
}

impl From<String> for PollFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            retry_after_seconds: 60,
        }
    }
}

fn host_url() -> String {
    std::env::var("RUNINATOR_ADAPTER_HOST_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8790".into())
        .trim_end_matches('/')
        .to_owned()
}

fn host_token() -> Result<String, String> {
    std::env::var("RUNINATOR_ADAPTER_HOST_TOKEN")
        .map_err(|_| "RUNINATOR_ADAPTER_HOST_TOKEN is not configured".into())
}

async fn invoke(kind: &str, request: AdapterPollRequest) -> Result<AdapterPollResponse, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| format!("adapter poll client could not be created: {error}"))?;
    let response = client
        .post(format!("{}/poll", host_url()))
        .bearer_auth(host_token()?)
        .json(&serde_json::json!({ "kind": kind, "request": request }))
        .send()
        .await
        .map_err(|error| format!("adapter host is unavailable: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "adapter host returned {}: {}",
            response.status(),
            response.text().await.unwrap_or_default()
        ));
    }
    response
        .json()
        .await
        .map_err(|error| format!("adapter host returned malformed poll output: {error}"))
}

fn interval_seconds(configuration: &runinator_models::value::Value) -> i64 {
    configuration
        .get("poll_interval_seconds")
        .and_then(|value| value.as_i64())
        .unwrap_or(60)
        .clamp(30, 3_600)
}

async fn poll_one<T: BackgroundEngineStore>(
    store: Arc<T>,
    pipelines: &PipelineOperations<T>,
    instance: &str,
    status: runinator_models::orchestration::AdapterPollStatus,
) -> Result<(), PollFailure> {
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
    let response = invoke(
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
            retry_after_seconds: response.retry_after_seconds.unwrap_or(60).clamp(30, 3_600) as i64,
        });
    }
    for mut event in response.events {
        event.validate_identity()?;
        adapters
            .resolve_correlation_alias(adapter.org_id, &mut event)
            .await?;
        if let Some(payload) = event.payload.as_object_mut() {
            if let Some(subject_revision) = event.subject_revision.clone() {
                payload.insert("subject_revision".into(), subject_revision.into());
            }
            if !event.provenance.is_null() {
                payload.insert("provenance".into(), event.provenance.clone());
            }
        }
        let pipeline_id = adapters.pipeline_for_event(&adapter, &event).await?;
        pipelines
            .process_ingress(
                pipeline_id,
                Some(adapter.org_id),
                PipelineIngressRequest {
                    source: format!("adapter:{}:{}", adapter.id, event.source),
                    event_id: event.delivery_id,
                    event_type: event.event_type,
                    correlation_key: event.correlation_key,
                    payload: event.payload,
                    provenance: event.provenance,
                    occurred_at: event.occurred_at,
                },
                Some((adapter.id, revision.revision)),
            )
            .await
            .map_err(|error| format!("{error:?}"))?;
    }
    let next = Utc::now() + TimeDelta::seconds(interval_seconds(&revision.configuration));
    store
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
    Ok(())
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
        let now = Utc::now();
        match store
            .claim_due_orchestration_adapter_polls(
                instance.clone(),
                now,
                now + TimeDelta::seconds(180),
                16,
            )
            .await
        {
            Ok(claims) => {
                for claim in claims {
                    if let Err(failure) =
                        poll_one(store.clone(), &pipelines, &instance, claim.clone()).await
                    {
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
            Err(err) => error!("failed to claim due adapter polls: {err}"),
        }
        tokio::select! { _ = shutdown.notified() => return, _ = tokio::time::sleep(Duration::from_secs(1)) => {} }
    }
}
