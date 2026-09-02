use super::*;
use runinator_models::{
    ingress_control::{
        BrokerIngressCapture, BrokerIngressCaptureRequest, BrokerIngressSession,
        BrokerIngressSessionMode, BrokerMessageDirection, BrokerMessageRecord,
        ExternalIngressCapture, ExternalIngressGate, ExternalIngressGateMode, IngressControlState,
    },
    orchestration::{IngressEvent, IngressTarget, IngressTargetKind},
    rbac::{ScopeKind, ScopeRef},
};

async fn ingress_db() -> SqliteDb {
    let path =
        std::env::temp_dir().join(format!("runinator-ingress-control-{}.db", Uuid::now_v7()));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    db
}

fn event(id: &str) -> IngressEvent {
    IngressEvent {
        source: "test".into(),
        event_id: id.into(),
        event_type: "changed".into(),
        correlation_key: "subject-1".into(),
        payload: runinator_models::json!({"id": id}),
        provenance: runinator_models::json!({"verified": true}),
        occurred_at: None,
    }
}

#[tokio::test]
async fn external_gate_queue_deduplicates_caps_and_claims_fifo() {
    let db = ingress_db().await;
    let target = IngressTarget {
        kind: IngressTargetKind::Workflow,
        id: Uuid::now_v7(),
    };
    let scope = ScopeRef::new(ScopeKind::Team, Some(Uuid::now_v7())).unwrap();
    db.put_external_ingress_gate(ExternalIngressGate {
        target: target.clone(),
        owner_scope: scope,
        mode: ExternalIngressGateMode::Paused,
        updated_by: None,
        updated_at: Utc::now(),
    })
    .await
    .unwrap();

    let first = db
        .capture_external_ingress(
            target.clone(),
            scope,
            ExternalIngressGateMode::Paused,
            event("one"),
            Utc::now(),
            2,
        )
        .await
        .unwrap();
    let first_id = match first {
        ExternalIngressCapture::Stored(record) => record.id,
        other => panic!("unexpected {other:?}"),
    };
    assert!(
        matches!(db.capture_external_ingress(target.clone(), scope, ExternalIngressGateMode::Paused, event("one"), Utc::now(), 2).await.unwrap(), ExternalIngressCapture::Duplicate(record) if record.id == first_id)
    );
    assert!(matches!(
        db.capture_external_ingress(
            target.clone(),
            scope,
            ExternalIngressGateMode::Paused,
            event("two"),
            Utc::now(),
            2
        )
        .await
        .unwrap(),
        ExternalIngressCapture::Stored(_)
    ));
    assert!(matches!(
        db.capture_external_ingress(
            target.clone(),
            scope,
            ExternalIngressGateMode::Paused,
            event("three"),
            Utc::now(),
            2
        )
        .await
        .unwrap(),
        ExternalIngressCapture::Full
    ));
    let claimed = db
        .claim_oldest_external_ingress_record(target, Uuid::now_v7(), Utc::now())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.id, first_id);
}

#[tokio::test]
async fn broker_sessions_are_exact_scoped_and_decisions_are_single_use() {
    let db = ingress_db().await;
    let org = ScopeRef::new(ScopeKind::Organization, Some(Uuid::now_v7())).unwrap();
    let team = ScopeRef::new(ScopeKind::Team, Some(Uuid::now_v7())).unwrap();
    db.put_broker_ingress_session(BrokerIngressSession {
        scope: org,
        mode: BrokerIngressSessionMode::HoldOrchestrationNudges,
        updated_by: None,
        updated_at: Utc::now(),
        expires_at: Utc::now() + chrono::Duration::seconds(15),
    })
    .await
    .unwrap();
    assert!(
        db.fetch_broker_ingress_session(team)
            .await
            .unwrap()
            .is_none()
    );

    let capture = db
        .capture_broker_ingress(BrokerIngressCaptureRequest {
            scope: org,
            delivery_id: Uuid::now_v7(),
            dedupe_key: "nudge-1".into(),
            command_kind: "orchestration_intent".into(),
            command: runinator_models::json!({"type": "orchestration_intent"}),
            hold: true,
            received_at: Utc::now(),
            capacity: 100,
        })
        .await
        .unwrap();
    let id = match capture {
        BrokerIngressCapture::Held(record) => record.id,
        other => panic!("unexpected {other:?}"),
    };
    let actor = Uuid::now_v7();
    assert!(
        db.decide_broker_ingress_record(id, IngressControlState::Approved, actor, Utc::now())
            .await
            .unwrap()
    );
    assert!(
        !db.decide_broker_ingress_record(id, IngressControlState::Dropped, actor, Utc::now())
            .await
            .unwrap()
    );
    assert_eq!(
        db.claim_approved_broker_ingress(Utc::now())
            .await
            .unwrap()
            .unwrap()
            .id,
        id
    );
}

#[tokio::test]
async fn broker_message_trace_filters_by_run_and_prunes_old_rows() {
    let db = ingress_db().await;
    let matching_run = Uuid::now_v7();
    let older_run = Uuid::now_v7();
    let now = Utc::now();

    for (workflow_run_id, occurred_at) in [
        (matching_run, now),
        (older_run, now - chrono::Duration::days(8)),
    ] {
        db.record_broker_message(BrokerMessageRecord {
            id: Uuid::now_v7(),
            channel: "effect".into(),
            direction: BrokerMessageDirection::Published,
            message_kind: "effect_command".into(),
            workflow_run_id: Some(workflow_run_id),
            delivery_id: None,
            dedupe_key: Some(workflow_run_id.to_string()),
            trace_id: None,
            payload: runinator_models::json!({"workflow_run_id": workflow_run_id}),
            occurred_at,
        })
        .await
        .unwrap();
    }

    let matching = db
        .fetch_broker_messages(Some(matching_run), None, Some("effect".into()), 20)
        .await
        .unwrap();
    assert_eq!(matching.len(), 1);
    assert_eq!(matching[0].workflow_run_id, Some(matching_run));

    assert_eq!(
        db.purge_broker_messages_before(now - chrono::Duration::days(7))
            .await
            .unwrap(),
        1
    );
}
