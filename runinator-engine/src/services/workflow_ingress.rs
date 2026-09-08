//! Workflow ingress shared by HTTP and recovered operator approvals.
use super::{
    CreateWorkflowRunRequest, IngressOperations, PipelineIngressError, PipelineIngressRequest,
    PipelineIngressResult, RunOperations,
};
use runinator_models::{
    ingress_control::{ExternalIngressCapture, ExternalIngressGateMode},
    interrupt::InterruptSource,
    orchestration::{
        IngressAction, IngressAdmissionClaim, IngressAdmissionStatus, IngressEvent,
        IngressEventDisposition, IngressInboxEntry, IngressLifecycle, IngressPolicy, IngressTarget,
        IngressTargetKind,
    },
    replicas::WorkflowRunProvenance,
};
use runinator_store::{
    RuntimeStore,
    roles::{
        AuthStore, FileStore, IngressStore, OrchestrationStore, RbacStore, RunStore, ScheduleStore,
        WorkflowVmStore,
    },
};
use std::sync::Arc;
use uuid::Uuid;
pub trait WorkflowIngressStore:
    RuntimeStore
    + WorkflowVmStore
    + RunStore
    + ScheduleStore
    + FileStore
    + AuthStore
    + RbacStore
    + IngressStore
    + OrchestrationStore
{
}
impl<T> WorkflowIngressStore for T where
    T: RuntimeStore
        + WorkflowVmStore
        + RunStore
        + ScheduleStore
        + FileStore
        + AuthStore
        + RbacStore
        + IngressStore
        + OrchestrationStore
{
}
pub struct WorkflowIngressContext<T> {
    pub db: Arc<T>,
    pub operations: Arc<RunOperations<T>>,
    pub caller_org_id: Option<Uuid>,
    pub actor_id: Option<Uuid>,
    pub workflow_id: Uuid,
    pub request: PipelineIngressRequest,
    pub provenance: WorkflowRunProvenance,
    pub bypass_gate: bool,
}

pub async fn process_workflow_ingress<T: WorkflowIngressStore>(
    context: WorkflowIngressContext<T>,
) -> Result<PipelineIngressResult, PipelineIngressError> {
    let WorkflowIngressContext {
        db,
        operations,
        caller_org_id,
        actor_id,
        workflow_id,
        request,
        provenance,
        bypass_gate,
    } = context;
    let workflow = match operations.fetch_workflow_definition(workflow_id).await {
        Ok(Some(workflow)) => workflow,
        Ok(None) => return not_found("workflow not found"),
        Err(err) => return api_error(err.to_string()),
    };
    let policy = match workflow.definition.metadata.get("ingress") {
        Some(value) => match serde_json::from_value::<IngressPolicy>(value.clone().into()) {
            Ok(policy) => policy,
            Err(error) => return bad_request(format!("invalid workflow ingress policy: {error}")),
        },
        None => return bad_request("workflow has no ingress policy"),
    };
    let event = IngressEvent {
        source: request.source,
        event_id: request.event_id,
        event_type: request.event_type,
        correlation_key: request.correlation_key,
        payload: request.payload,
        provenance: request.provenance,
        occurred_at: request.occurred_at,
    };
    let ingress = IngressOperations::new(db.clone());
    let requested_target = IngressTarget {
        kind: IngressTargetKind::Workflow,
        id: workflow_id,
    };
    match ingress.gate(requested_target.clone()).await {
        Ok(Some(gate)) if gate.mode != ExternalIngressGateMode::Disabled => {
            if bypass_gate {
                // An operator-approved event is already the durable copy captured by this gate.
            } else {
                let owner_scope = match ingress.owner_scope_for_target(&requested_target).await {
                    Ok(scope) => scope,
                    Err(err) => return api_error(err.to_string()),
                };
                return match ingress
                    .capture_for_review(requested_target, owner_scope, gate.mode, event)
                    .await
                {
                    Ok(
                        ExternalIngressCapture::Stored(record)
                        | ExternalIngressCapture::Duplicate(record),
                    ) => Err(PipelineIngressError::Held(Box::new(record))),
                    Ok(ExternalIngressCapture::Full) => Err(PipelineIngressError::Full),
                    Err(err) => api_error(err.to_string()),
                };
            }
        }
        Ok(_) => {}
        Err(err) => return api_error(err.to_string()),
    }
    let org_id = workflow.org_id.or(caller_org_id);
    let mut admission = match ingress
        .fetch(org_id, policy.scope.clone(), event.correlation_key.clone())
        .await
    {
        Ok(value) => value,
        Err(err) => return api_error(err.to_string()),
    };
    let mut start_record = None;
    if admission.is_none() {
        match ingress
            .claim_start(org_id, requested_target.clone(), policy.clone(), &event)
            .await
        {
            Ok(Some(IngressAdmissionClaim::Acquired(value))) => {
                start_record = match ingress
                    .persist_event(&value, &event, IngressEventDisposition::Started, false)
                    .await
                {
                    Ok(record) => Some(record.entry),
                    Err(err) => {
                        if let Some(id) = value.id {
                            let _ = ingress.release_unbound(id).await;
                        }
                        return api_error(err.to_string());
                    }
                };
                admission = Some(value);
            }
            Ok(Some(IngressAdmissionClaim::Existing(value))) => admission = Some(value),
            Ok(None) => {
                return bad_request(
                    "ingress event has no configured unbound start route; no run was started",
                );
            }
            Err(err) => return bad_request(err.to_string()),
        }
    }
    let mut admission = admission
        .ok_or_else(|| PipelineIngressError::Internal("ingress admission unresolved".into()))?;
    let admission_id = admission
        .id
        .ok_or_else(|| PipelineIngressError::Internal("ingress admission missing id".into()))?;
    if start_record.is_none() {
        match ingress.duplicate(admission_id, &event).await {
            Ok(Some(entry))
                if entry.workflow_run_id.is_some()
                    || !matches!(
                        entry.disposition,
                        IngressEventDisposition::Started | IngressEventDisposition::Requeued
                    ) =>
            {
                return ingress_event_reply(&entry, true, "duplicate ingress event");
            }
            Ok(Some(entry)) => start_record = Some(entry),
            Ok(None) => {}
            Err(err) => return api_error(err.to_string()),
        }
        if admission.target.kind != IngressTargetKind::Workflow
            || admission.target.id != workflow_id
        {
            return conflict(
                "this scope and correlation key is owned by a different ingress target",
            );
        }
        let snapshot_policy: IngressPolicy =
            match serde_json::from_value(admission.policy.clone().into()) {
                Ok(value) => value,
                Err(err) => return api_error(format!("stored ingress policy is invalid: {err}")),
            };
        let lifecycle = match admission.status {
            IngressAdmissionStatus::Active => IngressLifecycle::Active,
            IngressAdmissionStatus::Terminal => IngressLifecycle::Terminal,
        };
        if start_record.is_none() {
            match snapshot_policy.action_for_payload(&event.event_type, lifecycle, &event.payload) {
                Some(IngressAction::Record) => {
                    return match ingress
                        .persist_event(&admission, &event, IngressEventDisposition::Recorded, false)
                        .await
                    {
                        Ok(record) => ingress_event_reply(
                            &record.entry,
                            record.duplicate,
                            "ingress event recorded",
                        ),
                        Err(err) => api_error(err.to_string()),
                    };
                }
                Some(IngressAction::Queue) if lifecycle == IngressLifecycle::Active => {
                    return match ingress
                        .persist_event(&admission, &event, IngressEventDisposition::Queued, true)
                        .await
                    {
                        Ok(record) => ingress_event_reply(
                            &record.entry,
                            record.duplicate,
                            "ingress event queued",
                        ),
                        Err(err) => api_error(err.to_string()),
                    };
                }
                Some(IngressAction::Interrupt) if lifecycle == IngressLifecycle::Active => {
                    let Some(run_id) = admission.workflow_run_id else {
                        return api_error(
                            "active ingress admission is not bound to a workflow run",
                        );
                    };
                    let record = match ingress
                        .persist_event(
                            &admission,
                            &event,
                            IngressEventDisposition::InterruptRequested,
                            false,
                        )
                        .await
                    {
                        Ok(record) => record,
                        Err(err) => return api_error(err.to_string()),
                    };
                    if record.duplicate {
                        return ingress_event_reply(
                            &record.entry,
                            true,
                            "duplicate workflow interrupt event",
                        );
                    }
                    let _ = ingress
                        .bind_event_workflow_run(record.entry.id, run_id)
                        .await;
                    return match operations
                        .request_interrupt(
                            run_id,
                            InterruptSource::External,
                            event.payload.clone(),
                            None,
                        )
                        .await
                    {
                        Ok(_) => ingress_event_reply(
                            &record.entry,
                            record.duplicate,
                            "workflow interrupt requested",
                        ),
                        Err(err) => bad_request(err.to_string()),
                    };
                }
                Some(IngressAction::Requeue) if lifecycle == IngressLifecycle::Terminal => {
                    match ingress
                        .requeue_terminal_event(&admission, &snapshot_policy, &event)
                        .await
                    {
                        Ok(Some(record)) if record.duplicate => {
                            return ingress_event_reply(
                                &record.entry,
                                true,
                                "duplicate terminal requeue event",
                            );
                        }
                        Ok(Some(record)) => {
                            admission = match ingress
                                .fetch(
                                    org_id,
                                    snapshot_policy.scope.clone(),
                                    event.correlation_key.clone(),
                                )
                                .await
                            {
                                Ok(Some(value)) => value,
                                Ok(None) => {
                                    return api_error("requeued ingress admission disappeared");
                                }
                                Err(err) => return api_error(err.to_string()),
                            };
                            start_record = Some(record.entry);
                        }
                        Ok(None) => {
                            return conflict(
                                "another ingress event already started the next generation",
                            );
                        }
                        Err(err) => return api_error(err.to_string()),
                    }
                }
                _ => {
                    let _ = ingress
                        .persist_event(&admission, &event, IngressEventDisposition::Rejected, false)
                        .await;
                    return conflict(
                        "ingress event has no configured route for the admission lifecycle; no run was started",
                    );
                }
            }
        }
    }
    let start_entry = start_record
        .ok_or_else(|| PipelineIngressError::Internal("ingress start record missing".into()))?;
    match operations
        .create_for_ingress(
            CreateWorkflowRunRequest {
                workflow_id: admission.target.id,
                parameters: event.payload.clone(),
                debug: false,
                name: Some(format!("ingress:{}", event.event_id)),
                provenance,
                file_ids: Vec::new(),
                org_id: workflow.org_id.or(caller_org_id),
                principal_id: actor_id,
            },
            start_entry.id,
        )
        .await
    {
        Ok(run) => match ingress.bind_workflow_run(admission_id, run.id).await {
            Ok(true) => {
                let _ = ingress
                    .bind_event_workflow_run(start_entry.id, run.id)
                    .await;
                let mut entry = start_entry;
                entry.workflow_run_id = Some(run.id);
                ingress_event_reply(&entry, false, "workflow ingress generation started")
            }
            Ok(false) => api_error("ingress admission could not be bound to the workflow run"),
            Err(err) => api_error(err.to_string()),
        },
        Err(err) => {
            let _ = ingress.release_unbound(admission_id).await;
            api_error(err.to_string())
        }
    }
}

fn ingress_event_reply(
    entry: &IngressInboxEntry,
    duplicate: bool,
    message: &str,
) -> Result<PipelineIngressResult, PipelineIngressError> {
    Ok(super::pipeline_ingress::result(entry, duplicate, message))
}
fn bad_request(message: impl Into<String>) -> Result<PipelineIngressResult, PipelineIngressError> {
    Err(PipelineIngressError::Invalid(message.into()))
}
fn not_found(message: impl Into<String>) -> Result<PipelineIngressResult, PipelineIngressError> {
    Err(PipelineIngressError::NotFound(message.into()))
}
fn conflict(message: impl Into<String>) -> Result<PipelineIngressResult, PipelineIngressError> {
    Err(PipelineIngressError::Conflict(message.into()))
}
fn api_error(message: impl Into<String>) -> Result<PipelineIngressResult, PipelineIngressError> {
    Err(PipelineIngressError::Internal(message.into()))
}
