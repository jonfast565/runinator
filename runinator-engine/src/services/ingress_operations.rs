//! Provider-neutral ingress admission before a workflow or pipeline run is created.

use std::sync::Arc;

use chrono::Utc;
use runinator_models::{
    errors::SendableError,
    orchestration::{
        IngressAction, IngressAdmission, IngressAdmissionClaim, IngressAdmissionStatus,
        IngressEvent, IngressEventDisposition, IngressEventRecord, IngressInboxEntry,
        IngressLifecycle, IngressPolicy, IngressTarget,
    },
    value::Value,
};
use runinator_store::RuntimeStore;
use runinator_store::roles::IngressStore;
use uuid::Uuid;

/// The admission boundary used by HTTP, broker, or other transport adapters before they start a
/// target. It deliberately does not know how the event was authenticated or which target kind it
/// receives.
#[derive(Clone)]
pub struct IngressOperations<T> {
    store: Arc<T>,
}

impl<T> IngressOperations<T> {
    pub fn new(store: Arc<T>) -> Self {
        Self { store }
    }
}

impl<T: IngressStore> IngressOperations<T> {
    /// Acquire a previously unbound correlation key when its policy says to start. `None` means
    /// this event has no unbound-start route; callers must not create a target run in that case.
    pub async fn claim_start(
        &self,
        org_id: Option<Uuid>,
        target: IngressTarget,
        policy: IngressPolicy,
        event: &IngressEvent,
    ) -> Result<Option<IngressAdmissionClaim>, SendableError> {
        policy.validate().map_err(|message| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                message,
            )) as SendableError
        })?;
        if event.source.trim().is_empty()
            || event.event_id.trim().is_empty()
            || event.correlation_key.trim().is_empty()
        {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "ingress source, event_id, and correlation_key are required",
            )));
        }
        if policy.action_for(&event.event_type, IngressLifecycle::Unbound)
            != Some(IngressAction::Start)
        {
            return Ok(None);
        }
        let now = Utc::now();
        let admission = IngressAdmission {
            id: None,
            org_id,
            scope: policy.scope.clone(),
            correlation_key: event.correlation_key.clone(),
            generation: 1,
            target,
            status: IngressAdmissionStatus::Active,
            workflow_run_id: None,
            pipeline_run_id: None,
            policy: serde_json::to_value(policy)
                .map(Value::from)
                .map_err(|error| Box::new(error) as SendableError)?,
            created_at: now,
            updated_at: now,
        };
        self.store
            .claim_ingress_admission(admission, Some(event.clone()))
            .await
            .map(Some)
    }

    pub async fn bind_workflow_run(
        &self,
        admission_id: Uuid,
        workflow_run_id: Uuid,
    ) -> Result<bool, SendableError> {
        self.store
            .bind_ingress_workflow_run(admission_id, workflow_run_id, Utc::now())
            .await
    }

    pub async fn bind_pipeline_run(
        &self,
        admission_id: Uuid,
        pipeline_run_id: Uuid,
    ) -> Result<bool, SendableError> {
        self.store
            .bind_ingress_pipeline_run(admission_id, pipeline_run_id, Utc::now())
            .await
    }

    pub async fn settle_workflow_run(&self, workflow_run_id: Uuid) -> Result<bool, SendableError> {
        self.store
            .settle_ingress_workflow_run(workflow_run_id, Utc::now())
            .await
    }

    pub async fn settle_pipeline_run(&self, pipeline_run_id: Uuid) -> Result<bool, SendableError> {
        self.store
            .settle_ingress_pipeline_run(pipeline_run_id, Utc::now())
            .await
    }

    pub async fn release_unbound(&self, admission_id: Uuid) -> Result<bool, SendableError> {
        self.store
            .release_unbound_ingress_admission(admission_id)
            .await
    }

    pub async fn fetch(
        &self,
        org_id: Option<Uuid>,
        scope: String,
        correlation_key: String,
    ) -> Result<Option<IngressAdmission>, SendableError> {
        self.store
            .fetch_ingress_admission(org_id, scope, correlation_key)
            .await
    }

    pub async fn timeline(
        &self,
        admission_id: Uuid,
    ) -> Result<Vec<IngressInboxEntry>, SendableError> {
        self.store.fetch_ingress_events(admission_id).await
    }

    pub async fn duplicate(
        &self,
        admission_id: Uuid,
        event: &IngressEvent,
    ) -> Result<Option<IngressInboxEntry>, SendableError> {
        self.store
            .fetch_ingress_event(admission_id, event.source.clone(), event.event_id.clone())
            .await
    }

    pub async fn persist_event(
        &self,
        admission: &IngressAdmission,
        event: &IngressEvent,
        disposition: IngressEventDisposition,
        queued: bool,
    ) -> Result<IngressEventRecord, SendableError> {
        let admission_id = admission.id.ok_or_else(|| {
            Box::new(std::io::Error::other("stored ingress admission has no id")) as SendableError
        })?;
        self.store
            .record_ingress_event(
                admission_id,
                admission.generation,
                event.clone(),
                disposition,
                queued,
                Utc::now(),
            )
            .await
    }

    pub async fn bind_event_workflow_run(
        &self,
        event_id: Uuid,
        run_id: Uuid,
    ) -> Result<bool, SendableError> {
        self.store
            .bind_ingress_event_result(event_id, Some(run_id), None, Utc::now())
            .await
    }

    pub async fn bind_event_pipeline_run(
        &self,
        event_id: Uuid,
        run_id: Uuid,
    ) -> Result<bool, SendableError> {
        self.store
            .bind_ingress_event_result(event_id, None, Some(run_id), Utc::now())
            .await
    }

    pub async fn requeue_terminal_event(
        &self,
        existing: &IngressAdmission,
        policy: &IngressPolicy,
        event: &IngressEvent,
    ) -> Result<Option<IngressEventRecord>, SendableError> {
        if existing.status != IngressAdmissionStatus::Terminal
            || policy.action_for(&event.event_type, IngressLifecycle::Terminal)
                != Some(IngressAction::Requeue)
        {
            return Ok(None);
        }
        let id = existing.id.ok_or_else(|| {
            Box::new(std::io::Error::other("stored ingress admission has no id")) as SendableError
        })?;
        self.store
            .requeue_ingress_event(
                id,
                existing.generation,
                existing.target.clone(),
                serde_json::to_value(policy)
                    .map(Value::from)
                    .map_err(|error| Box::new(error) as SendableError)?,
                event.clone(),
                Utc::now(),
            )
            .await
    }
}

impl<T: IngressStore + RuntimeStore> IngressOperations<T> {
    /// Persist an active/terminal event without changing the admission's owner or generation.
    pub async fn record_event(
        &self,
        admission: &IngressAdmission,
        event: &IngressEvent,
    ) -> Result<(), SendableError> {
        self.persist_event(admission, event, IngressEventDisposition::Recorded, false)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runinator_database::sqlite::SqliteDb;
    use runinator_models::orchestration::{IngressRoute, IngressTargetKind};
    use runinator_store::DatabaseImpl;

    #[tokio::test]
    async fn one_unbound_start_claim_wins_for_a_shared_scope_and_key() {
        let path = std::env::temp_dir().join(format!("runinator-ingress-{}.db", Uuid::new_v4()));
        let db = Arc::new(SqliteDb::new(path.to_str().unwrap()).await.unwrap());
        db.run_init_scripts(&Vec::new()).await.unwrap();
        let service = IngressOperations::new(db);
        let policy = IngressPolicy {
            scope: "release.lifecycle".into(),
            routes: vec![
                IngressRoute {
                    event_type: "created".into(),
                    lifecycle: IngressLifecycle::Unbound,
                    action: IngressAction::Start,
                    predicates: vec![],
                    intent: None,
                },
                IngressRoute {
                    event_type: "reopened".into(),
                    lifecycle: IngressLifecycle::Terminal,
                    action: IngressAction::Requeue,
                    predicates: vec![],
                    intent: None,
                },
            ],
        };
        let event = IngressEvent {
            source: "example".into(),
            event_id: "evt-42".into(),
            event_type: "created".into(),
            correlation_key: "release-42".into(),
            payload: Value::Object(Default::default()),
            occurred_at: None,
        };
        let target = IngressTarget {
            kind: IngressTargetKind::Pipeline,
            id: Uuid::now_v7(),
        };
        let admission_id = match service
            .claim_start(None, target.clone(), policy.clone(), &event)
            .await
            .unwrap()
        {
            Some(IngressAdmissionClaim::Acquired(admission)) => admission.id.unwrap(),
            _ => panic!("first claim must acquire"),
        };
        assert!(matches!(
            service
                .claim_start(None, target.clone(), policy.clone(), &event)
                .await
                .unwrap(),
            Some(IngressAdmissionClaim::Existing(_))
        ));
        let pipeline_run_id = Uuid::now_v7();
        assert!(
            service
                .bind_pipeline_run(admission_id, pipeline_run_id)
                .await
                .unwrap()
        );
        assert!(service.settle_pipeline_run(pipeline_run_id).await.unwrap());
        assert!(!service.settle_pipeline_run(pipeline_run_id).await.unwrap());
        let terminal = service
            .store
            .fetch_ingress_admission(None, "release.lifecycle".into(), "release-42".into())
            .await
            .unwrap()
            .unwrap();
        let reopened = IngressEvent {
            event_id: "evt-43".into(),
            event_type: "reopened".into(),
            ..event
        };
        let first = service
            .requeue_terminal_event(&terminal, &policy, &reopened)
            .await
            .unwrap()
            .unwrap();
        assert!(!first.duplicate);
        let duplicate = service
            .requeue_terminal_event(&terminal, &policy, &reopened)
            .await
            .unwrap()
            .unwrap();
        assert!(duplicate.duplicate);
        let different = IngressEvent {
            event_id: "evt-other".into(),
            ..reopened
        };
        assert!(
            service
                .requeue_terminal_event(&terminal, &policy, &different)
                .await
                .unwrap()
                .is_none()
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn durable_dedup_and_fifo_promotion_survive_released_startup_claim() {
        let path =
            std::env::temp_dir().join(format!("runinator-ingress-fifo-{}.db", Uuid::new_v4()));
        let db = Arc::new(SqliteDb::new(path.to_str().unwrap()).await.unwrap());
        db.run_init_scripts(&Vec::new()).await.unwrap();
        let service = IngressOperations::new(db.clone());
        let policy = IngressPolicy {
            scope: "deploy.lifecycle".into(),
            routes: vec![
                IngressRoute {
                    event_type: "created".into(),
                    lifecycle: IngressLifecycle::Unbound,
                    action: IngressAction::Start,
                    predicates: vec![],
                    intent: None,
                },
                IngressRoute {
                    event_type: "updated".into(),
                    lifecycle: IngressLifecycle::Active,
                    action: IngressAction::Queue,
                    predicates: vec![],
                    intent: None,
                },
            ],
        };
        let initial = IngressEvent {
            source: "test".into(),
            event_id: "one".into(),
            event_type: "created".into(),
            correlation_key: "deploy-7".into(),
            payload: Value::Null,
            occurred_at: None,
        };
        let target = IngressTarget {
            kind: IngressTargetKind::Pipeline,
            id: Uuid::now_v7(),
        };
        let admission = match service
            .claim_start(None, target, policy, &initial)
            .await
            .unwrap()
            .unwrap()
        {
            IngressAdmissionClaim::Acquired(value) => value,
            _ => panic!("first start must acquire"),
        };
        let initial_again = service
            .persist_event(
                &admission,
                &initial,
                IngressEventDisposition::Started,
                false,
            )
            .await
            .unwrap();
        assert!(initial_again.duplicate);
        let run_one = Uuid::now_v7();
        service
            .bind_pipeline_run(admission.id.unwrap(), run_one)
            .await
            .unwrap();
        let queued_one = IngressEvent {
            event_id: "two".into(),
            event_type: "updated".into(),
            ..initial.clone()
        };
        let queued_two = IngressEvent {
            event_id: "three".into(),
            event_type: "updated".into(),
            ..initial
        };
        let first = service
            .persist_event(
                &admission,
                &queued_one,
                IngressEventDisposition::Queued,
                true,
            )
            .await
            .unwrap();
        let second = service
            .persist_event(
                &admission,
                &queued_two,
                IngressEventDisposition::Queued,
                true,
            )
            .await
            .unwrap();
        assert_eq!(first.entry.queue_position, Some(1));
        assert_eq!(second.entry.queue_position, Some(2));

        let promotion = db
            .settle_and_promote_ingress_pipeline_run(run_one, Utc::now())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(promotion.event.event_id, "two");
        assert!(
            db.release_ingress_promotion(promotion.claim_token, Utc::now())
                .await
                .unwrap()
        );
        let retried = db
            .claim_queued_ingress_event(Utc::now())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retried.event.event_id, "two");
        let run_two = Uuid::now_v7();
        assert!(
            db.bind_ingress_pipeline_run(admission.id.unwrap(), run_two, Utc::now())
                .await
                .unwrap()
        );
        assert!(
            db.bind_ingress_event_result(retried.event.id, None, Some(run_two), Utc::now())
                .await
                .unwrap()
        );
        let next = db
            .settle_and_promote_ingress_pipeline_run(run_two, Utc::now())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(next.event.event_id, "three");
        let timeline = db
            .fetch_ingress_events(admission.id.unwrap())
            .await
            .unwrap();
        assert_eq!(timeline.len(), 3);
        let _ = std::fs::remove_file(path);
    }
}
