//! Recoverable adapter publication, delivery admission, and approved ingress application.
use crate::{
    engine::BackgroundEngineStore,
    events::EventSender,
    services::{
        AdapterRoutingError, PipelineIngressRequest, PipelineOperations, RunOperations,
        WorkflowIngressContext, process_workflow_ingress,
    },
};
use chrono::Utc;
use runinator_broker_core::{Broker, BrokerError, EffectMessage};
use runinator_models::{
    ingress_control::ExternalIngressRecord,
    orchestration::IngressTargetKind,
    replicas::{TriggerActorType, TriggerSourceKind, WorkflowRunProvenance},
    value::Value,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::Notify;
use tracing::warn;
use uuid::Uuid;

pub async fn run_adapter_control_loop<T: BackgroundEngineStore>(
    store: Arc<T>,
    broker: Arc<dyn Broker>,
    events: EventSender,
    shutdown: Arc<Notify>,
) {
    let pipelines = PipelineOperations::new(store.clone(), broker.clone(), events.clone(), None);
    let runs = Arc::new(RunOperations::new(
        store.clone(),
        broker.clone(),
        events,
        None,
    ));
    let mut last_cleanup = Utc::now();
    loop {
        if let Err(error) = store.expire_adapter_poll_attempts(Utc::now()).await {
            warn!(%error,"could not expire adapter poll attempts");
        }
        for _ in 0..16 {
            let token = Uuid::now_v7();
            match store
                .claim_adapter_poll_publication(token, Utc::now())
                .await
            {
                Ok(Some(dispatch)) => {
                    let result = broker
                        .publish_effect(EffectMessage {
                            dedupe_key: Some(format!("adapter-poll:{}", dispatch.id)),
                            command: dispatch.command,
                            enqueued_at: dispatch.created_at,
                            expires_at: Some(dispatch.deadline_at),
                        })
                        .await;
                    let published = matches!(result, Ok(()) | Err(BrokerError::Duplicate(_)));
                    if let Err(error) = store
                        .finish_adapter_poll_publication(dispatch.id, token, published)
                        .await
                    {
                        warn!(%error,"could not finish adapter publication");
                    }
                    if !published {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    warn!(%error,"could not claim adapter publication");
                    break;
                }
            }
        }
        for _ in 0..16 {
            let token = Uuid::now_v7();
            match store.claim_adapter_delivery(token, Utc::now()).await {
                Ok(Some(mut record)) => {
                    record.error = None;
                    if let Err(error) =
                        crate::services::process_delivery(store.clone(), &pipelines, &mut record)
                            .await
                    {
                        record.state = match &error {
                            AdapterRoutingError::Rejected(_) => "rejected",
                            AdapterRoutingError::Unavailable(_) => "failed",
                        }
                        .into();
                        record.error = Some(error.to_string());
                    }
                    record.updated_at = Utc::now();
                    if let Err(error) = store.finish_adapter_delivery(record, token).await {
                        warn!(%error,"could not finish adapter delivery");
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    warn!(%error,"could not claim adapter delivery");
                    break;
                }
            }
        }
        for _ in 0..16 {
            let token = Uuid::now_v7();
            match store
                .claim_approved_external_ingress(token, Utc::now())
                .await
            {
                Ok(Some(record)) => {
                    let outcome =
                        apply_external(store.clone(), runs.clone(), &pipelines, &record).await;
                    let error = outcome.as_ref().err().cloned();
                    match store
                        .finish_approved_external_ingress(
                            record.id,
                            token,
                            error.clone(),
                            Utc::now(),
                        )
                        .await
                    {
                        Ok(true) => {
                            if let Some(id) =
                                record.adapter.and_then(|origin| origin.delivery_record_id)
                            {
                                match store.fetch_adapter_delivery(id).await {
                                    Ok(Some(mut delivery)) => {
                                        delivery.state =
                                            if error.is_some() { "failed" } else { "applied" }
                                                .into();
                                        delivery.error = error;
                                        delivery.outcome = outcome.unwrap_or(Value::Null);
                                        delivery.updated_at = Utc::now();
                                        if let Err(error) =
                                            store.update_adapter_delivery(delivery).await
                                        {
                                            warn!(%error,"could not correlate reviewed delivery");
                                        }
                                    }
                                    Ok(None) => {}
                                    Err(error) => warn!(%error,"could not find reviewed delivery"),
                                }
                            }
                        }
                        Ok(false) => {
                            warn!(record_id=%record.id,"approved ingress application lease was lost")
                        }
                        Err(error) => {
                            warn!(%error,"could not finish approved ingress; lease will recover")
                        }
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    warn!(%error,"could not claim approved ingress");
                    break;
                }
            }
        }
        if Utc::now() - last_cleanup >= chrono::Duration::minutes(1) {
            if let Err(error) = store
                .purge_adapter_diagnostics(Utc::now() - chrono::Duration::days(7))
                .await
            {
                warn!(%error,"could not prune adapter diagnostics");
            }
            last_cleanup = Utc::now();
        }
        tokio::select! {_=shutdown.notified()=>return,_=tokio::time::sleep(Duration::from_millis(250))=>{}}
    }
}

async fn apply_external<T: BackgroundEngineStore>(
    store: Arc<T>,
    runs: Arc<RunOperations<T>>,
    pipelines: &PipelineOperations<T>,
    record: &ExternalIngressRecord,
) -> Result<Value, String> {
    let event = record.event.clone();
    let request = PipelineIngressRequest {
        source: event.source,
        event_id: event.event_id,
        event_type: event.event_type,
        correlation_key: event.correlation_key,
        payload: event.payload,
        provenance: event.provenance,
        occurred_at: event.occurred_at,
    };
    let outcome = match record.target.kind {
        IngressTargetKind::Pipeline => {
            pipelines
                .process_approved_ingress(
                    record.target.id,
                    record.caller_org_id,
                    request,
                    record.adapter,
                )
                .await
        }
        IngressTargetKind::Workflow => {
            process_workflow_ingress(WorkflowIngressContext {
                db: store,
                operations: runs,
                caller_org_id: record.caller_org_id,
                actor_id: record.reviewed_by,
                workflow_id: record.target.id,
                request,
                bypass_gate: true,
                provenance: WorkflowRunProvenance {
                    source_kind: Some(TriggerSourceKind::Api),
                    actor_type: Some(TriggerActorType::User),
                    actor_replica_id: None,
                    actor_display_name: Some("ingress-control".into()),
                    request_host: None,
                    request_ip: None,
                    metadata: record.event.provenance.clone(),
                },
            })
            .await
        }
    }
    .map_err(|error| format!("{error:?}"))?;
    serde_json::to_value(outcome)
        .map(Value::from)
        .map_err(|error| error.to_string())
}
