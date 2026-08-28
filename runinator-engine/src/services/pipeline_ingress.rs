//! Transport-neutral pipeline ingress shared by HTTP webhooks and durable polling adapters.

use chrono::Utc;
use runinator_models::{
    orchestration::{
        INGRESS_CORRELATION_KEY_LIMIT, INGRESS_DELIVERY_ID_LIMIT, INGRESS_EVENT_TYPE_LIMIT,
        IngressAction, IngressAdmissionClaim, IngressAdmissionStatus, IngressEvent,
        IngressEventDisposition, IngressInboxEntry, IngressLifecycle, IngressPolicy, IngressTarget,
        IngressTargetKind,
    },
    value::Value,
};
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, IngressStore, OrchestrationStore, ScheduleStore, WorkflowVmStore},
};
use uuid::Uuid;

use super::{IngressOperations, OrchestrationOperations, PipelineOperations};

#[derive(Debug, Clone)]
pub struct PipelineIngressRequest {
    pub source: String,
    pub event_id: String,
    pub event_type: String,
    pub correlation_key: String,
    pub payload: Value,
    pub provenance: Value,
    pub occurred_at: Option<chrono::DateTime<Utc>>,
}

impl PipelineIngressRequest {
    /// Reject an identity no backend can store before it reaches an insert. `scope` and
    /// `correlation_key` share one exact unique key that mysql caps at 3072 utf8mb4 bytes, so an
    /// oversized value is a dialect-dependent insert failure rather than a clean rejection.
    /// `source` here is the composed `adapter:<id>:<source>` form, which is why its bound is wider
    /// than the adapter-side one.
    fn validate_identity(&self) -> Result<(), String> {
        for (name, value, limit) in [
            ("source", self.source.as_str(), 191usize),
            (
                "event_id",
                self.event_id.as_str(),
                INGRESS_DELIVERY_ID_LIMIT,
            ),
            (
                "event_type",
                self.event_type.as_str(),
                INGRESS_EVENT_TYPE_LIMIT,
            ),
            (
                "correlation_key",
                self.correlation_key.as_str(),
                INGRESS_CORRELATION_KEY_LIMIT,
            ),
        ] {
            if value.trim().is_empty() {
                return Err(format!("ingress {name} must not be empty"));
            }
            if value.len() > limit {
                return Err(format!(
                    "ingress {name} is longer than the {limit} characters every backend can store"
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PipelineIngressResult {
    pub admission_id: Uuid,
    pub generation: i64,
    pub disposition: String,
    pub duplicate: bool,
    pub queue_position: Option<i64>,
    pub workflow_run_id: Option<Uuid>,
    pub pipeline_run_id: Option<Uuid>,
    pub orchestration_binding_id: Option<Uuid>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum PipelineIngressError {
    NotFound(String),
    Invalid(String),
    Conflict(String),
    Internal(String),
}

impl PipelineIngressError {
    fn internal(error: impl std::fmt::Display) -> Self {
        Self::Internal(error.to_string())
    }
}

fn result(entry: &IngressInboxEntry, duplicate: bool, message: &str) -> PipelineIngressResult {
    PipelineIngressResult {
        admission_id: entry.admission_id,
        generation: entry.promoted_generation.unwrap_or(entry.generation),
        disposition: format!("{:?}", entry.disposition).to_ascii_lowercase(),
        duplicate,
        queue_position: entry.queue_position,
        workflow_run_id: entry.workflow_run_id,
        pipeline_run_id: entry.pipeline_run_id,
        orchestration_binding_id: None,
        message: message.into(),
    }
}

impl<T> PipelineOperations<T>
where
    T: DefinitionStore
        + RuntimeStore
        + ScheduleStore
        + WorkflowVmStore
        + IngressStore
        + OrchestrationStore,
{
    pub async fn process_ingress(
        &self,
        pipeline_id: Uuid,
        caller_org_id: Option<Uuid>,
        request: PipelineIngressRequest,
        adapter: Option<(Uuid, i64)>,
    ) -> Result<PipelineIngressResult, PipelineIngressError> {
        let pipeline = self
            .fetch(pipeline_id)
            .await
            .map_err(PipelineIngressError::internal)?
            .ok_or_else(|| PipelineIngressError::NotFound("pipeline not found".into()))?;
        let policy: IngressPolicy = serde_json::from_value(
            pipeline
                .metadata
                .get("ingress")
                .ok_or_else(|| {
                    PipelineIngressError::Invalid("pipeline has no ingress policy".into())
                })?
                .clone()
                .into(),
        )
        .map_err(|error| {
            PipelineIngressError::Invalid(format!("invalid pipeline ingress policy: {error}"))
        })?;
        request
            .validate_identity()
            .map_err(PipelineIngressError::Invalid)?;
        let event = IngressEvent {
            source: request.source,
            event_id: request.event_id,
            event_type: request.event_type,
            correlation_key: request.correlation_key,
            payload: request.payload,
            provenance: request.provenance,
            occurred_at: request.occurred_at,
        };
        let ingress = IngressOperations::new(self.store.clone());
        let target = IngressTarget {
            kind: IngressTargetKind::Pipeline,
            id: pipeline_id,
        };
        let org_id = pipeline.org_id.or(caller_org_id);
        let mut admission = ingress
            .fetch(org_id, policy.scope.clone(), event.correlation_key.clone())
            .await
            .map_err(PipelineIngressError::internal)?;
        let mut start_record = None;
        if admission.is_none() {
            match ingress
                .claim_start(org_id, target, policy.clone(), &event)
                .await
                .map_err(|error| PipelineIngressError::Invalid(error.to_string()))?
            {
                Some(IngressAdmissionClaim::Acquired(value)) => {
                    match ingress
                        .persist_event(&value, &event, IngressEventDisposition::Started, false)
                        .await
                    {
                        Ok(record) => start_record = Some(record.entry),
                        Err(error) => {
                            if let Some(id) = value.id {
                                let _ = ingress.release_unbound(id).await;
                            }
                            return Err(PipelineIngressError::internal(error));
                        }
                    }
                    admission = Some(value);
                }
                Some(IngressAdmissionClaim::Existing(value)) => admission = Some(value),
                None => {
                    return Err(PipelineIngressError::Invalid(
                        "ingress event has no configured unbound start route; no run was started"
                            .into(),
                    ));
                }
            }
        }
        // these were `expect`s when this lived in an http handler, where a panic cost one request.
        // the same code now also runs on the engine's poll loop, where a panic takes down every
        // background loop with it, so an impossible state degrades to an error instead.
        let mut admission = admission.ok_or_else(|| {
            PipelineIngressError::Internal("ingress admission was not resolved".into())
        })?;
        let admission_id = admission.id.ok_or_else(|| {
            PipelineIngressError::Internal("resolved ingress admission has no stored id".into())
        })?;
        if start_record.is_none() {
            if let Some(entry) = ingress
                .duplicate(admission_id, &event)
                .await
                .map_err(PipelineIngressError::internal)?
            {
                return Ok(result(&entry, true, "duplicate ingress event"));
            }
            if admission.target.kind != IngressTargetKind::Pipeline
                || admission.target.id != pipeline_id
            {
                return Err(PipelineIngressError::Conflict(
                    "this scope and correlation key is owned by a different ingress target".into(),
                ));
            }
            let snapshot: IngressPolicy = serde_json::from_value(admission.policy.clone().into())
                .map_err(PipelineIngressError::internal)?;
            let lifecycle = match admission.status {
                IngressAdmissionStatus::Active => IngressLifecycle::Active,
                IngressAdmissionStatus::Terminal => IngressLifecycle::Terminal,
            };
            if !snapshot
                .dispatches_for(&event.event_type, lifecycle, &event.payload)
                .is_empty()
            {
                let record = ingress
                    .persist_event(&admission, &event, IngressEventDisposition::Recorded, false)
                    .await
                    .map_err(PipelineIngressError::internal)?;
                return Ok(result(
                    &record.entry,
                    record.duplicate,
                    "orchestration intent event accepted",
                ));
            }
            match snapshot.action_for_payload(&event.event_type, lifecycle, &event.payload) {
                Some(IngressAction::Record) => {
                    let record = ingress
                        .persist_event(&admission, &event, IngressEventDisposition::Recorded, false)
                        .await
                        .map_err(PipelineIngressError::internal)?;
                    return Ok(result(
                        &record.entry,
                        record.duplicate,
                        "ingress event recorded",
                    ));
                }
                Some(IngressAction::Queue) if lifecycle == IngressLifecycle::Active => {
                    let record = ingress
                        .persist_event(&admission, &event, IngressEventDisposition::Queued, true)
                        .await
                        .map_err(PipelineIngressError::internal)?;
                    return Ok(result(
                        &record.entry,
                        record.duplicate,
                        "ingress event queued",
                    ));
                }
                Some(IngressAction::Interrupt) if lifecycle == IngressLifecycle::Active => {
                    let run_id = admission.pipeline_run_id.ok_or_else(|| {
                        PipelineIngressError::Internal(
                            "active ingress admission is not bound to a pipeline run".into(),
                        )
                    })?;
                    let record = ingress
                        .persist_event(
                            &admission,
                            &event,
                            IngressEventDisposition::InterruptRequested,
                            false,
                        )
                        .await
                        .map_err(PipelineIngressError::internal)?;
                    if record.duplicate {
                        return Ok(result(
                            &record.entry,
                            true,
                            "duplicate pipeline interrupt event",
                        ));
                    }
                    let _ = ingress
                        .bind_event_pipeline_run(record.entry.id, run_id)
                        .await;
                    self.cancel_run(run_id)
                        .await
                        .map_err(|error| PipelineIngressError::Invalid(error.to_string()))?;
                    return Ok(result(
                        &record.entry,
                        false,
                        "pipeline and active members canceled",
                    ));
                }
                Some(IngressAction::Requeue) if lifecycle == IngressLifecycle::Terminal => {
                    match ingress
                        .requeue_terminal_event(&admission, &snapshot, &event)
                        .await
                        .map_err(PipelineIngressError::internal)?
                    {
                        Some(record) if record.duplicate => {
                            return Ok(result(
                                &record.entry,
                                true,
                                "duplicate terminal requeue event",
                            ));
                        }
                        Some(record) => {
                            admission = ingress
                                .fetch(
                                    org_id,
                                    snapshot.scope.clone(),
                                    event.correlation_key.clone(),
                                )
                                .await
                                .map_err(PipelineIngressError::internal)?
                                .ok_or_else(|| {
                                    PipelineIngressError::Internal(
                                        "requeued ingress admission disappeared".into(),
                                    )
                                })?;
                            start_record = Some(record.entry);
                        }
                        None => {
                            return Err(PipelineIngressError::Conflict(
                                "another ingress event already started the next generation".into(),
                            ));
                        }
                    }
                }
                _ => {
                    let _ = ingress
                        .persist_event(&admission, &event, IngressEventDisposition::Rejected, false)
                        .await;
                    return Err(PipelineIngressError::Conflict("ingress event has no configured route for the admission lifecycle; no run was started".into()));
                }
            }
        }
        let start_entry = start_record.ok_or_else(|| {
            PipelineIngressError::Internal("ingress start event was not recorded".into())
        })?;
        if pipeline.metadata.get("orchestration").is_some() {
            let orchestrations = OrchestrationOperations::new(self.store.clone());
            let binding = orchestrations
                .admit_with_adapter(&admission, &pipeline, adapter)
                .await
                .map_err(PipelineIngressError::internal)?
                .ok_or_else(|| {
                    PipelineIngressError::Internal(
                        "managed orchestration policy disappeared".into(),
                    )
                })?;
            if let Some((adapter_id, _)) = adapter {
                self.store
                    .mark_orchestration_adapter_admitted(adapter_id, Utc::now())
                    .await
                    .map_err(PipelineIngressError::internal)?;
            }
            let mut reply = result(
                &start_entry,
                false,
                "managed orchestration generation admitted",
            );
            reply.orchestration_binding_id = Some(binding.id);
            reply.disposition = "started".into();
            return Ok(reply);
        }
        match self
            .create_run(
                admission.target.id,
                event.payload.clone(),
                None,
                Some(format!("ingress:{}", event.event_id)),
                None,
            )
            .await
        {
            Ok(run) => match ingress
                .bind_pipeline_run(admission_id, run.id)
                .await
                .map_err(PipelineIngressError::internal)?
            {
                true => {
                    let _ = ingress
                        .bind_event_pipeline_run(start_entry.id, run.id)
                        .await;
                    let mut entry = start_entry;
                    entry.pipeline_run_id = Some(run.id);
                    Ok(result(&entry, false, "pipeline ingress generation started"))
                }
                false => Err(PipelineIngressError::Internal(
                    "ingress admission could not be bound to the pipeline run".into(),
                )),
            },
            Err(error) => {
                let _ = ingress.release_unbound(admission_id).await;
                Err(PipelineIngressError::internal(error))
            }
        }
    }
}
