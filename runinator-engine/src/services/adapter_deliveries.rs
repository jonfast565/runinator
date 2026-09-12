//! Capture before admission so every normalized delivery has a recoverable outcome.
use super::{
    AdapterOperations, AdapterRoutingError, PipelineIngressError, PipelineIngressRequest,
    PipelineOperations,
};
use chrono::Utc;
use runinator_models::{
    adapter_control::{AdapterDeliveryRecord, AdapterOrigin},
    auth::ResourceType,
    errors::SendableError,
    ingress_control::ExternalIngressGateMode,
    orchestration::NormalizedAdapterEvent,
    value::Value,
};
use runinator_store::{
    RuntimeStore,
    roles::{
        DefinitionStore, IngressStore, OrchestrationStore, RbacStore, ScheduleStore,
        WorkflowVmStore,
    },
};
use std::sync::Arc;
use uuid::Uuid;

impl<T: OrchestrationStore> AdapterOperations<T> {
    pub async fn capture_delivery(
        &self,
        origin: AdapterOrigin,
        attempt_id: Option<Uuid>,
        event: Option<NormalizedAdapterEvent>,
        error: Option<String>,
    ) -> Result<AdapterDeliveryRecord, SendableError> {
        let id = Uuid::now_v7();
        self.store
            .record_adapter_delivery(AdapterDeliveryRecord {
                id,
                origin: AdapterOrigin {
                    delivery_record_id: Some(id),
                    ..origin
                },
                attempt_id,
                event,
                state: if error.is_some() {
                    "rejected"
                } else {
                    "pending"
                }
                .into(),
                hold_mode: None,
                error,
                preview: Value::Null,
                outcome: Value::Null,
                approved: false,
                received_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .await
    }
}

pub(crate) async fn process_delivery<T>(
    store: Arc<T>,
    pipelines: &PipelineOperations<T>,
    record: &mut AdapterDeliveryRecord,
) -> Result<(), AdapterRoutingError>
where
    T: runinator_store::roles::AuthStore
        + OrchestrationStore
        + DefinitionStore
        + IngressStore
        + RuntimeStore
        + ScheduleStore
        + WorkflowVmStore
        + RbacStore,
{
    let operations = AdapterOperations::new(store.clone());
    let adapter = operations
        .fetch(record.origin.adapter_id)
        .await
        .map_err(unavailable)?
        .filter(|adapter| adapter.enabled)
        .ok_or_else(|| AdapterRoutingError::Rejected("adapter is missing or disabled".into()))?;
    let event = record.event.clone().ok_or_else(|| {
        AdapterRoutingError::Rejected("delivery has no verified event to retry".into())
    })?;
    record.preview = operations.preview_event(&adapter, &event).await?.into();
    let gate = store
        .adapter_inspection(adapter.id)
        .await
        .map_err(unavailable)?
        .mode;
    if !record.approved && gate != ExternalIngressGateMode::Disabled {
        record.state = "held".into();
        record.hold_mode = Some(gate);
        return Ok(());
    }
    let event = operations.prepare_event(adapter.org_id, event).await?;
    let pipeline_id = operations.pipeline_for_event(&adapter, &event).await?;
    if !runinator_store::resource_access::resource_can_consume(
        store.as_ref(),
        ResourceType::Pipeline,
        pipeline_id,
        ResourceType::OrchestrationAdapter,
        adapter.id,
    )
    .await
    .map_err(unavailable)?
    {
        return Err(AdapterRoutingError::Rejected(
            "pipeline is not permitted to consume this adapter".into(),
        ));
    }
    let outcome = pipelines
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
            Some(record.origin),
        )
        .await;
    match outcome {
        Ok(value) => {
            record.state = "applied".into();
            record.outcome = serde_json::to_value(value).map_err(unavailable)?.into();
        }
        Err(PipelineIngressError::Held(held)) => {
            record.state = "held_at_pipeline".into();
            record.outcome =
                serde_json::json!({"review_record_id":held.id,"pipeline_id":pipeline_id}).into();
        }
        Err(PipelineIngressError::Full) => {
            return Err(AdapterRoutingError::Unavailable(
                "pipeline review queue is full; delivery retained for retry".into(),
            ));
        }
        Err(PipelineIngressError::Internal(message)) => {
            return Err(AdapterRoutingError::Unavailable(message));
        }
        Err(error) => return Err(AdapterRoutingError::Rejected(format!("{error:?}"))),
    }
    Ok(())
}

fn unavailable(error: impl std::fmt::Display) -> AdapterRoutingError {
    AdapterRoutingError::Unavailable(error.to_string())
}

/// Redact diagnostic copies without changing the payload used for execution.
pub fn redact_adapter_diagnostic(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(values) => {
            for (key, value) in values {
                if [
                    "authorization",
                    "password",
                    "access_token",
                    "api_token",
                    "secret",
                    "token",
                    "cookie",
                ]
                .iter()
                .any(|name| key.eq_ignore_ascii_case(name))
                {
                    *value = serde_json::Value::String("[redacted]".into());
                } else {
                    redact_adapter_diagnostic(value);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                redact_adapter_diagnostic(value)
            }
        }
        _ => {}
    }
}
