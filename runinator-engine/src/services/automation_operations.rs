//! application service for durable automation records, gates, and idempotency leases.

use std::sync::Arc;

use runinator_models::{
    errors::SendableError,
    orchestration::{
        IdempotencyClaim, IdempotencyClaimRequest, IdempotencyCompleteRequest,
        IdempotencyReleaseRequest,
    },
    value::Value,
    workflows::WorkflowRun,
};
use runinator_store::{
    RuntimeStore,
    roles::{AutomationStore, DeliveryStore},
};
use uuid::Uuid;

use crate::repository;

/// Provides automation persistence operations to transport adapters.
#[derive(Clone)]
pub struct AutomationOperations<T> {
    store: Arc<T>,
}

impl<T> AutomationOperations<T> {
    pub fn new(store: Arc<T>) -> Self {
        Self { store }
    }
}

impl<T: RuntimeStore + AutomationStore + DeliveryStore> AutomationOperations<T> {
    pub async fn list_records(
        &self,
        record_type: &str,
        workflow_run_id: Option<Uuid>,
        external_item_id: Option<Uuid>,
    ) -> Result<Vec<Value>, SendableError> {
        repository::fetch_automation_records(
            self.store.as_ref(),
            record_type,
            workflow_run_id,
            external_item_id,
        )
        .await
    }

    pub async fn create_record(
        &self,
        record_type: &str,
        record: Value,
    ) -> Result<Value, SendableError> {
        repository::create_automation_record(self.store.as_ref(), record_type, record).await
    }

    pub async fn list_gates(
        &self,
        workflow_run_id: Option<Uuid>,
        status: Option<String>,
    ) -> Result<Vec<Value>, SendableError> {
        repository::fetch_gates(self.store.as_ref(), workflow_run_id, status).await
    }

    pub async fn fetch_gate(&self, gate_id: Uuid) -> Result<Option<Value>, SendableError> {
        repository::fetch_gate(self.store.as_ref(), gate_id).await
    }

    pub async fn create_gate(&self, record: Value) -> Result<Value, SendableError> {
        repository::create_gate(self.store.as_ref(), record).await
    }

    pub async fn delete_gate(&self, gate_id: Uuid) -> Result<bool, SendableError> {
        repository::delete_gate(self.store.as_ref(), gate_id).await
    }

    pub async fn delete_record(
        &self,
        record_type: &str,
        record_id: Uuid,
    ) -> Result<bool, SendableError> {
        repository::delete_automation_record(self.store.as_ref(), record_type, record_id).await
    }

    pub async fn fetch_idempotency_key(
        &self,
        scope: String,
        key: String,
    ) -> Result<Option<Value>, SendableError> {
        repository::fetch_idempotency_key(self.store.as_ref(), scope, key).await
    }

    pub async fn put_idempotency_key(
        &self,
        scope: String,
        key: String,
        result: Value,
    ) -> Result<Value, SendableError> {
        repository::put_idempotency_key(self.store.as_ref(), scope, key, result).await
    }

    pub async fn claim_idempotency_key(
        &self,
        request: IdempotencyClaimRequest,
    ) -> Result<IdempotencyClaim, SendableError> {
        repository::claim_idempotency_key(
            self.store.as_ref(),
            request.scope,
            request.key,
            request.owner_node_run_id,
            request.lease_seconds,
        )
        .await
    }

    pub async fn complete_idempotency_key(
        &self,
        request: IdempotencyCompleteRequest,
    ) -> Result<bool, SendableError> {
        repository::complete_idempotency_key(
            self.store.as_ref(),
            request.scope,
            request.key,
            request.owner_node_run_id,
            request.result,
        )
        .await
    }

    pub async fn release_idempotency_key(
        &self,
        request: IdempotencyReleaseRequest,
    ) -> Result<bool, SendableError> {
        repository::release_idempotency_key(
            self.store.as_ref(),
            request.scope,
            request.key,
            request.owner_node_run_id,
        )
        .await
    }

    pub async fn workflow_run(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Option<WorkflowRun>, SendableError> {
        repository::fetch_workflow_run(self.store.as_ref(), workflow_run_id).await
    }
}
