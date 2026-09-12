//! One regression per durable adapter-control gap, shared across SQL dialects.
use super::*;
use runinator_models::{adapter_control::*, ingress_control::*, rbac::ScopeRef};
use runinator_store::roles::AdapterPollDispatch;

fn review_request() -> ExternalIngressCaptureRequest {
    ExternalIngressCaptureRequest {
        target: IngressTarget {
            kind: IngressTargetKind::Pipeline,
            id: Uuid::now_v7(),
        },
        owner_scope: ScopeRef::new(runinator_models::rbac::ScopeKind::Platform, None).unwrap(),
        gate_mode: ExternalIngressGateMode::Review,
        event: IngressEvent {
            source: "adapter:test".into(),
            event_id: Uuid::now_v7().to_string(),
            event_type: "changed".into(),
            correlation_key: "subject".into(),
            payload: Value::Null,
            provenance: Value::Null,
            occurred_at: None,
        },
        adapter: Some(AdapterOrigin {
            adapter_id: Uuid::now_v7(),
            revision: 7,
            delivery_record_id: Some(Uuid::now_v7()),
        }),
        now: Utc::now(),
        capacity: 100,
    }
}
async fn held<T: DatabaseImpl>(db: &T) -> ExternalIngressRecord {
    match db
        .capture_external_ingress_request(review_request())
        .await
        .unwrap()
    {
        ExternalIngressCapture::Stored(record) => record,
        other => panic!("unexpected capture {other:?}"),
    }
}

pub(super) async fn held_origin_survives_approval<T: DatabaseImpl>(db: &T) {
    let record = held(db).await;
    let origin = record.adapter.unwrap();
    let org = Uuid::now_v7();
    db.approve_external_ingress(record.id, Uuid::now_v7(), Some(org))
        .await
        .unwrap();
    let token = Uuid::now_v7();
    let claimed = db
        .claim_approved_external_ingress(token, Utc::now())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.id, record.id);
    let recovered = claimed.adapter.unwrap();
    assert_eq!(
        (
            recovered.adapter_id,
            recovered.revision,
            recovered.delivery_record_id
        ),
        (origin.adapter_id, 7, origin.delivery_record_id)
    );
    assert_eq!(claimed.caller_org_id, Some(org));
    assert!(
        db.finish_approved_external_ingress(record.id, token, None, Utc::now())
            .await
            .unwrap()
    );
}

pub(super) async fn approval_recovers_expired_lease_and_retries_failure<T: DatabaseImpl>(db: &T) {
    let record = held(db).await;
    let actor = Uuid::now_v7();
    let now = Utc::now();
    db.approve_external_ingress(record.id, actor, None)
        .await
        .unwrap();
    let first = Uuid::now_v7();
    db.claim_approved_external_ingress(first, now)
        .await
        .unwrap()
        .unwrap();
    assert!(
        db.claim_approved_external_ingress(Uuid::now_v7(), now)
            .await
            .unwrap()
            .is_none()
    );
    let second = Uuid::now_v7();
    assert_eq!(
        db.claim_approved_external_ingress(second, now + Duration::seconds(301))
            .await
            .unwrap()
            .unwrap()
            .id,
        record.id
    );
    assert!(
        !db.finish_approved_external_ingress(record.id, first, None, now)
            .await
            .unwrap()
    );
    assert!(
        db.finish_approved_external_ingress(record.id, second, Some("transient".into()), now)
            .await
            .unwrap()
    );
    assert!(
        db.approve_external_ingress(record.id, actor, None)
            .await
            .unwrap()
    );
    let third = Uuid::now_v7();
    db.claim_approved_external_ingress(third, now)
        .await
        .unwrap()
        .unwrap();
    assert!(
        db.finish_approved_external_ingress(record.id, third, None, now)
            .await
            .unwrap()
    );
}

pub(super) async fn delivery_journal_and_broker_trace_are_adapter_scoped<T: DatabaseImpl>(db: &T) {
    let now = Utc::now();
    let adapter_id = Uuid::now_v7();
    let attempt = Uuid::now_v7();
    let record = AdapterDeliveryRecord {
        id: Uuid::now_v7(),
        origin: AdapterOrigin {
            adapter_id,
            revision: 1,
            delivery_record_id: None,
        },
        attempt_id: Some(attempt),
        event: None,
        state: "rejected".into(),
        hold_mode: None,
        error: Some("invalid signature".into()),
        preview: Value::Null,
        outcome: Value::Null,
        approved: false,
        received_at: now,
        updated_at: now,
    };
    let stored = db.record_adapter_delivery(record.clone()).await.unwrap();
    assert_eq!(
        db.record_adapter_delivery(record).await.unwrap().id,
        stored.id
    );
    assert_eq!(
        db.fetch_adapter_deliveries(adapter_id, 10)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        db.fetch_adapter_deliveries(Uuid::now_v7(), 10)
            .await
            .unwrap()
            .is_empty()
    );
    let mut paused = stored.clone();
    paused.id = Uuid::now_v7();
    paused.state = "held".into();
    paused.hold_mode = Some(ExternalIngressGateMode::Paused);
    paused.error = None;
    paused.received_at += chrono::Duration::seconds(1);
    paused.updated_at = paused.received_at;
    db.record_adapter_delivery(paused.clone()).await.unwrap();
    let mut next = paused.clone();
    next.id = Uuid::now_v7();
    next.received_at += chrono::Duration::seconds(1);
    next.updated_at = next.received_at;
    db.record_adapter_delivery(next).await.unwrap();
    assert!(!db.decide_adapter_delivery(paused.id, true).await.unwrap());
    assert_eq!(
        db.release_paused_adapter_deliveries(adapter_id, 1)
            .await
            .unwrap(),
        (1, 1)
    );
    assert_eq!(
        db.release_paused_adapter_deliveries(adapter_id, 10)
            .await
            .unwrap(),
        (1, 0)
    );
    db.record_broker_message(BrokerMessageRecord {
        id: Uuid::now_v7(),
        adapter_id: Some(adapter_id),
        poll_attempt_id: Some(attempt),
        channel: "effect_result".into(),
        direction: BrokerMessageDirection::Received,
        message_kind: "effect_result".into(),
        workflow_run_id: None,
        delivery_id: None,
        dedupe_key: None,
        trace_id: Some(attempt),
        payload: Value::Null,
        occurred_at: now,
    })
    .await
    .unwrap();
    let messages = db
        .fetch_broker_messages(None, None, Some(adapter_id), None, 10)
        .await
        .unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].poll_attempt_id, Some(attempt));
    assert!(messages[0].workflow_run_id.is_none());
}

pub(super) async fn reducer_pause_step_and_resume<T: DatabaseImpl>(db: &T) {
    let id = Uuid::now_v7();
    db.set_orchestration_debug_control(
        id,
        OrchestrationDebugControl {
            paused: true,
            steps: 0,
        },
    )
    .await
    .unwrap();
    assert!(!db.take_orchestration_debug_permit(id).await.unwrap());
    db.set_orchestration_debug_control(
        id,
        OrchestrationDebugControl {
            paused: true,
            steps: 1,
        },
    )
    .await
    .unwrap();
    assert!(db.take_orchestration_debug_permit(id).await.unwrap());
    assert!(!db.take_orchestration_debug_permit(id).await.unwrap());
    db.set_orchestration_debug_control(id, OrchestrationDebugControl::default())
        .await
        .unwrap();
    assert!(db.take_orchestration_debug_permit(id).await.unwrap());
}

pub(super) async fn poll_publication_recovers_and_retains_fenced_attempts<T: DatabaseImpl>(db: &T) {
    let now = Utc::now();
    let adapter_id = Uuid::now_v7();
    db.create_orchestration_adapter(
        NewAdapterDefinition {
            id: adapter_id,
            org_id: Uuid::now_v7(),
            name: "attempts".into(),
            kind: "github".into(),
            kind_version: "1".into(),
            transport: AdapterTransport::Polling,
            endpoint_identity: adapter_id.to_string(),
            configuration: Value::Null,
            authentication: Default::default(),
            identity_configuration: Value::Null,
            actor_id: None,
        },
        now,
    )
    .await
    .unwrap();
    let mut ids = Vec::new();
    for _ in 0..2 {
        let id = Uuid::now_v7();
        ids.push(id);
        let command = EffectCommand {
            version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
            command_id: Uuid::now_v7(),
            effect_id: id,
            workflow_run_id: adapter_id,
            continuation_id: Uuid::nil(),
            attempt: 1,
            request: WorkflowEffectRequest::Action {
                provider: "__runinator_adapter".into(),
                function: "poll".into(),
                input: Value::Null,
                timeout_seconds: Some(120),
                retry: Default::default(),
                tags: vec![],
                required_labels: BTreeMap::new(),
                workspace_affinity: None,
                execution_profile: None,
                idempotency_key: None,
                function_binding: None,
            },
            executor: runinator_comm::EffectExecutor::Provider,
            target: runinator_comm::ActionTarget::Any,
            trace_id: id,
            trace_context: Default::default(),
            idempotency_key: id.to_string(),
            notification_delivery_id: None,
        };
        db.insert_orchestration_adapter_poll_dispatch(AdapterPollDispatch {
            id,
            adapter_id,
            adapter_revision: 1,
            profile_id: Uuid::now_v7(),
            claim_owner: id.to_string(),
            command,
            state: "queued".into(),
            dry_run: true,
            deadline_at: now + Duration::seconds(300),
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap();
    }
    let token = Uuid::now_v7();
    let first = db
        .claim_adapter_poll_publication(token, now)
        .await
        .unwrap()
        .unwrap();
    let recovered_token = Uuid::now_v7();
    let recovered = db
        .claim_adapter_poll_publication(recovered_token, now + Duration::seconds(31))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered.id, first.id);
    db.finish_adapter_poll_publication(first.id, token, true)
        .await
        .unwrap();
    assert_eq!(
        db.fetch_orchestration_adapter_poll_dispatch(first.id)
            .await
            .unwrap()
            .unwrap()
            .state,
        "publishing"
    );
    db.finish_adapter_poll_publication(first.id, recovered_token, true)
        .await
        .unwrap();
    db.expire_adapter_poll_attempts(now + Duration::seconds(301))
        .await
        .unwrap();
    assert!(
        !db.finish_adapter_poll_attempt(first.id, "succeeded".into(), Value::Null, None, now)
            .await
            .unwrap()
    );
    let attempts = db
        .fetch_adapter_poll_attempts(adapter_id, 10)
        .await
        .unwrap();
    assert_eq!(attempts.len(), 2);
    assert!(attempts.iter().all(|attempt| attempt.state == "expired"));
}
