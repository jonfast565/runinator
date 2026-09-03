//! notification-owned provider-effect outbox persistence.

use super::*;
use runinator_comm::{EffectCommand, EffectExecutor};
use runinator_models::{
    notifications::{NotificationChannel, NotificationDeliveryStatus},
    workflow_vm::{WORKFLOW_EFFECT_PROTOCOL_VERSION, WorkflowEffectRequest},
};

fn inbox_notification(org_id: Uuid, dedupe_key: &str) -> NewNotification {
    NewNotification {
        org_id: Some(org_id),
        source_resource_type: None,
        source_resource_id: None,
        workflow_run_id: None,
        workflow_node_id: None,
        channel: "in_app".into(),
        severity: "info".into(),
        title: "scoped inbox event".into(),
        body: None,
        target: None,
        metadata: Value::Null,
        dedupe_key: Some(dedupe_key.into()),
    }
}

#[tokio::test]
async fn notifications_dedupe_by_tenant_and_keep_personal_receipts() {
    let path = std::env::temp_dir().join(format!(
        "runinator-notification-receipts-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let first_org = Uuid::now_v7();
    let second_org = Uuid::now_v7();
    let first = db
        .create_notification_if_absent(&inbox_notification(first_org, "same-alias"))
        .await
        .unwrap()
        .unwrap();
    let duplicate = db
        .create_notification_if_absent(&inbox_notification(first_org, "same-alias"))
        .await
        .unwrap();
    let other_tenant = db
        .create_notification_if_absent(&inbox_notification(second_org, "same-alias"))
        .await
        .unwrap();
    assert!(duplicate.is_none());
    assert!(other_tenant.is_some());

    let reader = Uuid::now_v7();
    let other_reader = Uuid::now_v7();
    db.mark_notification_read(Some(first_org), first.id, reader)
        .await
        .unwrap();
    assert!(
        db.fetch_notification(Some(first_org), first.id, reader)
            .await
            .unwrap()
            .unwrap()
            .read_at
            .is_some()
    );
    assert!(
        db.fetch_notification(Some(first_org), first.id, other_reader)
            .await
            .unwrap()
            .unwrap()
            .read_at
            .is_none()
    );

    assert!(
        db.delete_notification(Some(first_org), first.id, reader)
            .await
            .unwrap()
    );
    assert!(
        db.fetch_notification(Some(first_org), first.id, reader)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.fetch_notification(Some(first_org), first.id, other_reader)
            .await
            .unwrap()
            .is_some()
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn notification_effect_outbox_claims_retries_and_marks_delivery_dispatched() {
    let path = std::env::temp_dir().join(format!(
        "runinator-notification-effect-outbox-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let notification = db
        .create_notification(&NewNotification {
            org_id: None,
            source_resource_type: None,
            source_resource_id: None,
            workflow_run_id: None,
            workflow_node_id: None,
            channel: "slack".into(),
            severity: "warning".into(),
            title: "delivery test".into(),
            body: Some("body".into()),
            target: Some("#ops".into()),
            metadata: Value::Null,
            dedupe_key: None,
        })
        .await
        .unwrap();
    let delivery_id = Uuid::now_v7();
    let command = EffectCommand {
        version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
        command_id: Uuid::now_v7(),
        effect_id: delivery_id,
        workflow_run_id: Uuid::nil(),
        continuation_id: Uuid::nil(),
        attempt: 0,
        request: WorkflowEffectRequest::Action {
            provider: "slack".into(),
            function: "send_message".into(),
            input: Value::Null,
            timeout_seconds: Some(30),
            retry: Default::default(),
            tags: Vec::new(),
            required_labels: Default::default(),
            workspace_affinity: None,
            execution_profile: None,
            idempotency_key: None,
            function_binding: None,
        },
        executor: EffectExecutor::Provider,
        target: Default::default(),
        trace_id: Uuid::now_v7(),
        trace_context: Default::default(),
        idempotency_key: format!("notification:{delivery_id}"),
        notification_delivery_id: Some(delivery_id),
    };
    db.create_notification_delivery(
        delivery_id,
        notification.id,
        None,
        NotificationChannel::Slack,
        Some("#ops".into()),
        command.clone(),
    )
    .await
    .unwrap();

    let now = Utc::now();
    let claimed = db
        .claim_pending_notification_effect_dispatches(
            "publisher-a".into(),
            now,
            now + Duration::seconds(30),
            10,
        )
        .await
        .unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].delivery_id, delivery_id);
    assert_eq!(
        claimed[0].command.notification_delivery_id,
        Some(delivery_id)
    );

    db.mark_notification_effect_dispatch_failed(delivery_id, "broker unavailable".into())
        .await
        .unwrap();
    let retried = db
        .claim_pending_notification_effect_dispatches(
            "publisher-b".into(),
            Utc::now(),
            Utc::now() + Duration::seconds(30),
            10,
        )
        .await
        .unwrap();
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0].attempts, 1);

    db.mark_notification_effect_dispatch_published(delivery_id)
        .await
        .unwrap();
    assert!(
        db.claim_pending_notification_effect_dispatches(
            "publisher-c".into(),
            Utc::now(),
            Utc::now() + Duration::seconds(30),
            10,
        )
        .await
        .unwrap()
        .is_empty()
    );
    let deliveries = db
        .fetch_notification_deliveries(notification.id)
        .await
        .unwrap();
    assert_eq!(deliveries[0].status, NotificationDeliveryStatus::Dispatched);
    assert_eq!(deliveries[0].attempts, 1);
}
