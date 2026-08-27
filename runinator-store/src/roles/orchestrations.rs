//! Durable correlated orchestration bindings and their reducer outboxes.

use std::collections::BTreeMap;
use std::future::Future;

use chrono::{DateTime, Utc};
use runinator_models::{
    errors::SendableError,
    orchestration::{
        NewOrchestrationBinding, OrchestrationBinding, OrchestrationCommand, OrchestrationEpoch,
        OrchestrationEventReduction, OrchestrationEvidence, OrchestrationPendingIntent,
        OrchestrationStatus,
    },
    value::Value,
};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct OrchestrationBindingUpdate {
    pub expected_version: i64,
    pub status: OrchestrationStatus,
    pub current_phase: Option<String>,
    pub current_attempt: i64,
    pub current_epoch: i64,
    pub restart_member: Option<String>,
    pub resume_existing_epoch: bool,
    pub subject_revision: Option<String>,
    pub resources: Value,
    pub budgets: BTreeMap<String, u32>,
    pub last_reduced_sequence: i64,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewOrchestrationEpoch {
    pub id: Uuid,
    pub binding_id: Uuid,
    pub epoch: i64,
    pub start_member: Option<String>,
    pub parameters: Value,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct NewOrchestrationCommand {
    pub id: Uuid,
    pub binding_id: Uuid,
    pub epoch: i64,
    pub command_type: String,
    pub operation_key: String,
    pub payload: Value,
}

/// Owns binding CAS, reducer leasing, immutable reductions/epochs, and the command outbox.
pub trait OrchestrationStore: Send + Sync + 'static {
    fn create_orchestration_binding(
        &self,
        binding: NewOrchestrationBinding,
    ) -> impl Future<Output = Result<OrchestrationBinding, SendableError>> + Send;

    fn fetch_orchestration_binding(
        &self,
        binding_id: Uuid,
    ) -> impl Future<Output = Result<Option<OrchestrationBinding>, SendableError>> + Send;

    fn fetch_orchestration_binding_for_admission(
        &self,
        admission_id: Uuid,
        generation: i64,
    ) -> impl Future<Output = Result<Option<OrchestrationBinding>, SendableError>> + Send;

    fn fetch_orchestration_bindings(
        &self,
        org_id: Option<Uuid>,
        status: Option<OrchestrationStatus>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<OrchestrationBinding>, SendableError>> + Send;

    /// Claim reducible bindings with an expiring lease. A binding can be returned again after
    /// expiry, so every mutation still uses its version as a compare-and-swap guard.
    fn claim_orchestration_bindings(
        &self,
        owner: String,
        now: DateTime<Utc>,
        leased_until: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<OrchestrationBinding>, SendableError>> + Send;

    fn update_orchestration_binding(
        &self,
        binding_id: Uuid,
        owner: String,
        update: OrchestrationBindingUpdate,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<OrchestrationBinding>, SendableError>> + Send;

    fn release_orchestration_binding_lease(
        &self,
        binding_id: Uuid,
        owner: String,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    fn create_orchestration_epoch(
        &self,
        epoch: NewOrchestrationEpoch,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<OrchestrationEpoch, SendableError>> + Send;

    fn bind_orchestration_epoch_run(
        &self,
        binding_id: Uuid,
        epoch: i64,
        pipeline_run_id: Uuid,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    fn fetch_orchestration_epochs(
        &self,
        binding_id: Uuid,
    ) -> impl Future<Output = Result<Vec<OrchestrationEpoch>, SendableError>> + Send;

    fn record_orchestration_reduction(
        &self,
        reduction: OrchestrationEventReduction,
    ) -> impl Future<Output = Result<OrchestrationEventReduction, SendableError>> + Send;

    fn fetch_orchestration_reductions(
        &self,
        binding_id: Uuid,
    ) -> impl Future<Output = Result<Vec<OrchestrationEventReduction>, SendableError>> + Send;

    fn upsert_orchestration_pending_intent(
        &self,
        intent: OrchestrationPendingIntent,
    ) -> impl Future<Output = Result<OrchestrationPendingIntent, SendableError>> + Send;

    fn fetch_due_orchestration_intents(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<OrchestrationPendingIntent>, SendableError>> + Send;

    fn fetch_orchestration_pending_intents(
        &self,
        binding_id: Uuid,
    ) -> impl Future<Output = Result<Vec<OrchestrationPendingIntent>, SendableError>> + Send;

    fn delete_orchestration_pending_intents_below(
        &self,
        binding_id: Uuid,
        priority: i32,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    fn delete_orchestration_pending_intent(
        &self,
        binding_id: Uuid,
        intent: String,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    fn enqueue_orchestration_command(
        &self,
        command: NewOrchestrationCommand,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<OrchestrationCommand, SendableError>> + Send;

    fn fetch_orchestration_commands(
        &self,
        binding_id: Uuid,
    ) -> impl Future<Output = Result<Vec<OrchestrationCommand>, SendableError>> + Send;

    fn claim_orchestration_commands(
        &self,
        owner: String,
        now: DateTime<Utc>,
        leased_until: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<OrchestrationCommand>, SendableError>> + Send;

    fn complete_orchestration_command(
        &self,
        command_id: Uuid,
        owner: String,
        succeeded: bool,
        result: Value,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    fn append_orchestration_evidence(
        &self,
        evidence: OrchestrationEvidence,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    fn fetch_orchestration_evidence(
        &self,
        binding_id: Uuid,
    ) -> impl Future<Output = Result<Vec<OrchestrationEvidence>, SendableError>> + Send;
}
