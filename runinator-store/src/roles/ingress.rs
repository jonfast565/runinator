//! Durable, provider-neutral ingress admission records.

use std::future::Future;

use chrono::{DateTime, Utc};
use runinator_models::{
    errors::SendableError,
    orchestration::{
        IngressAdmission, IngressAdmissionClaim, IngressEvent, IngressEventDisposition,
        IngressEventRecord, IngressInboxEntry, IngressPromotion, IngressTarget,
    },
    value::Value,
};
use uuid::Uuid;

/// Owns the atomic `(organization, scope, correlation key)` admission boundary.
pub trait IngressStore: Send + Sync + 'static {
    /// Insert an active admission if no admission exists for this key.  Concurrent callers receive
    /// the same existing record, so only the acquired caller can create a workflow/pipeline run.
    fn claim_ingress_admission(
        &self,
        admission: IngressAdmission,
        initial_event: Option<IngressEvent>,
    ) -> impl Future<Output = Result<IngressAdmissionClaim, SendableError>> + Send;

    fn fetch_ingress_admission(
        &self,
        org_id: Option<Uuid>,
        scope: String,
        correlation_key: String,
    ) -> impl Future<Output = Result<Option<IngressAdmission>, SendableError>> + Send;

    /// Insert the event once and return the original row on a durable deduplication hit.
    fn record_ingress_event(
        &self,
        admission_id: Uuid,
        generation: i64,
        event: IngressEvent,
        disposition: IngressEventDisposition,
        queued: bool,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<IngressEventRecord, SendableError>> + Send;

    fn fetch_ingress_events(
        &self,
        admission_id: Uuid,
    ) -> impl Future<Output = Result<Vec<IngressInboxEntry>, SendableError>> + Send;

    fn fetch_ingress_event(
        &self,
        admission_id: Uuid,
        source: String,
        event_id: String,
    ) -> impl Future<Output = Result<Option<IngressInboxEntry>, SendableError>> + Send;

    fn bind_ingress_event_result(
        &self,
        event_id: Uuid,
        workflow_run_id: Option<Uuid>,
        pipeline_run_id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Attach the workflow run created by the caller that acquired this admission. The update is
    /// conditional on its target kind, preventing a workflow start from binding a pipeline slot.
    fn bind_ingress_workflow_run(
        &self,
        admission_id: Uuid,
        workflow_run_id: Uuid,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Attach the pipeline run created by the caller that acquired this admission.
    fn bind_ingress_pipeline_run(
        &self,
        admission_id: Uuid,
        pipeline_run_id: Uuid,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Mark the active admission owning this workflow run terminal. This is idempotent so terminal
    /// notifications can be retried by the engine.
    fn settle_ingress_workflow_run(
        &self,
        workflow_run_id: Uuid,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Mark the active admission owning this pipeline run terminal.
    fn settle_ingress_pipeline_run(
        &self,
        pipeline_run_id: Uuid,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Mark a bound generation terminal and atomically claim/promote its oldest queued child.
    fn settle_and_promote_ingress_workflow_run(
        &self,
        workflow_run_id: Uuid,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<IngressPromotion>, SendableError>> + Send;

    fn settle_and_promote_ingress_pipeline_run(
        &self,
        pipeline_run_id: Uuid,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<IngressPromotion>, SendableError>> + Send;

    /// Return a failed promoted event to the FIFO head and restore the admission to terminal.
    fn release_ingress_promotion(
        &self,
        claim_token: Uuid,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Retry queue startup after a prior claim was released. Claims one terminal admission's FIFO
    /// head without allowing a later child to overtake it.
    fn claim_queued_ingress_event(
        &self,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<IngressPromotion>, SendableError>> + Send;

    /// Remove an acquired admission only while no target run has been bound. Used when the start
    /// operation fails after claiming, so a transient failure cannot permanently block the key.
    fn release_unbound_ingress_admission(
        &self,
        admission_id: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Atomically record/deduplicate a terminal requeue event and advance the expected generation.
    fn requeue_ingress_event(
        &self,
        admission_id: Uuid,
        expected_generation: i64,
        target: IngressTarget,
        policy: Value,
        event: IngressEvent,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<IngressEventRecord>, SendableError>> + Send;
}
