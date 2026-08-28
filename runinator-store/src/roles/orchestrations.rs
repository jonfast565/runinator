//! Durable correlated orchestration bindings and their reducer outboxes.

use std::collections::BTreeMap;
use std::future::Future;

use chrono::{DateTime, Utc};
use runinator_models::{
    errors::SendableError,
    orchestration::{
        AdapterDefinition, AdapterPollStatus, AdapterRevision, AdapterTransport, ExternalOperation,
        ExternalOperationStatus, NewOrchestrationBinding, OrchestrationBinding,
        OrchestrationCommand, OrchestrationCorrelationAlias, OrchestrationEpoch,
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
pub struct NewOrchestrationCorrelationAlias {
    pub id: Uuid,
    pub binding_id: Uuid,
    pub generation: i64,
    pub org_id: Option<Uuid>,
    pub source: String,
    pub scope: String,
    pub correlation_key: String,
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

#[derive(Debug, Clone)]
pub struct NewAdapterDefinition {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub kind: String,
    pub kind_version: String,
    pub transport: AdapterTransport,
    pub endpoint_identity: String,
    pub configuration: Value,
    pub secret_bindings: BTreeMap<String, Uuid>,
    pub identity_configuration: Value,
    pub actor_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct NewAdapterRevision {
    pub id: Uuid,
    pub adapter_id: Uuid,
    pub expected_revision: i64,
    pub kind_version: String,
    pub transport: AdapterTransport,
    pub configuration: Value,
    pub secret_bindings: BTreeMap<String, Uuid>,
    pub identity_configuration: Value,
    pub actor_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct ExternalOperationUpdate {
    pub status: ExternalOperationStatus,
    pub attempt: i64,
    pub ambiguous: bool,
    pub provenance: Value,
    pub receipt: Value,
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

    fn upsert_orchestration_correlation_alias(
        &self,
        alias: NewOrchestrationCorrelationAlias,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<OrchestrationCorrelationAlias, SendableError>> + Send;

    fn fetch_orchestration_correlation_alias(
        &self,
        org_id: Option<Uuid>,
        source: String,
        scope: String,
        correlation_key: String,
    ) -> impl Future<Output = Result<Option<OrchestrationCorrelationAlias>, SendableError>> + Send;

    fn fetch_orchestration_correlation_aliases(
        &self,
        binding_id: Uuid,
    ) -> impl Future<Output = Result<Vec<OrchestrationCorrelationAlias>, SendableError>> + Send;

    fn delete_orchestration_correlation_alias(
        &self,
        binding_id: Uuid,
        alias_id: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Resolve the current managed binding that owns a workflow effect. Historical epochs are
    /// deliberately excluded so a late dispatch or receipt can only be recorded as stale.
    fn fetch_current_orchestration_binding_for_workflow_run(
        &self,
        workflow_run_id: Uuid,
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

    /// Atomically apply a leased binding CAS and remove the coalesced intent that produced it.
    /// If the CAS loses, the pending intent remains available for the winning reducer to consume.
    fn consume_orchestration_pending_intent(
        &self,
        binding_id: Uuid,
        intent: String,
        priority: i32,
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

    /// Settle an immutable epoch after its pipeline run reaches terminal. Repeated settlement is
    /// idempotent and cannot rewrite a terminal epoch.
    fn settle_orchestration_epoch(
        &self,
        binding_id: Uuid,
        epoch: i64,
        status: String,
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

    /// Release a failed but safely replayable internal command back to the durable queue.
    fn retry_orchestration_command(
        &self,
        command_id: Uuid,
        owner: String,
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

    fn create_orchestration_adapter(
        &self,
        adapter: NewAdapterDefinition,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<(AdapterDefinition, AdapterRevision), SendableError>> + Send;

    fn fetch_orchestration_adapter(
        &self,
        adapter_id: Uuid,
    ) -> impl Future<Output = Result<Option<AdapterDefinition>, SendableError>> + Send;

    fn fetch_orchestration_adapter_by_endpoint(
        &self,
        endpoint_identity: String,
    ) -> impl Future<Output = Result<Option<AdapterDefinition>, SendableError>> + Send;

    fn fetch_orchestration_adapters(
        &self,
        org_id: Uuid,
    ) -> impl Future<Output = Result<Vec<AdapterDefinition>, SendableError>> + Send;

    fn fetch_orchestration_adapter_revision(
        &self,
        adapter_id: Uuid,
        revision: i64,
    ) -> impl Future<Output = Result<Option<AdapterRevision>, SendableError>> + Send;

    fn fetch_orchestration_adapter_revisions(
        &self,
        adapter_id: Uuid,
    ) -> impl Future<Output = Result<Vec<AdapterRevision>, SendableError>> + Send;

    fn create_orchestration_adapter_revision(
        &self,
        revision: NewAdapterRevision,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<(AdapterDefinition, AdapterRevision)>, SendableError>> + Send;

    fn set_orchestration_adapter_enabled(
        &self,
        adapter_id: Uuid,
        enabled: bool,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<AdapterDefinition>, SendableError>> + Send;

    fn mark_orchestration_adapter_admitted(
        &self,
        adapter_id: Uuid,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    fn delete_orchestration_adapter(
        &self,
        adapter_id: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    fn claim_due_orchestration_adapter_polls(
        &self,
        instance_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<AdapterPollStatus>, SendableError>> + Send;

    fn fetch_orchestration_adapter_poll_status(
        &self,
        adapter_id: Uuid,
    ) -> impl Future<Output = Result<Option<AdapterPollStatus>, SendableError>> + Send;

    fn complete_orchestration_adapter_poll(
        &self,
        adapter_id: Uuid,
        instance_id: String,
        revision: i64,
        checkpoint: Value,
        next_poll_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    fn fail_orchestration_adapter_poll(
        &self,
        adapter_id: Uuid,
        instance_id: String,
        next_poll_at: DateTime<Utc>,
        error: String,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    fn create_external_operation(
        &self,
        operation: ExternalOperation,
    ) -> impl Future<Output = Result<ExternalOperation, SendableError>> + Send;

    fn fetch_external_operation(
        &self,
        operation_id: Uuid,
    ) -> impl Future<Output = Result<Option<ExternalOperation>, SendableError>> + Send;

    fn fetch_external_operation_for_effect(
        &self,
        effect_id: Uuid,
    ) -> impl Future<Output = Result<Option<ExternalOperation>, SendableError>> + Send;

    fn fetch_external_operations(
        &self,
        binding_id: Uuid,
    ) -> impl Future<Output = Result<Vec<ExternalOperation>, SendableError>> + Send;

    fn update_external_operation(
        &self,
        operation_id: Uuid,
        update: ExternalOperationUpdate,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<ExternalOperation>, SendableError>> + Send;
}
