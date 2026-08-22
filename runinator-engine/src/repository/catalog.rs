use super::*;
use uuid::Uuid;

pub async fn upsert_catalog_item<T: DefinitionStore>(
    db: &T,
    item: Value,
) -> Result<Value, SendableError> {
    db.upsert_catalog_item(item).await
}

pub async fn fetch_catalog_items<T: DefinitionStore>(
    db: &T,
    item_type: Option<String>,
) -> Result<Vec<Value>, SendableError> {
    db.fetch_catalog_items(item_type).await
}

pub async fn fetch_catalog_item<T: DefinitionStore>(
    db: &T,
    uri: String,
) -> Result<Option<Value>, SendableError> {
    db.fetch_catalog_item(uri).await
}

pub async fn delete_catalog_item<T: DefinitionStore>(
    db: &T,
    uri: &str,
) -> Result<bool, SendableError> {
    db.delete_catalog_item(uri.to_string()).await
}

pub async fn create_automation_record<T: RuntimeStore>(
    db: &T,
    record_type: &str,
    record: Value,
) -> Result<Value, SendableError> {
    db.create_automation_record(record_type.into(), record)
        .await
}

pub async fn fetch_automation_records<T: AutomationStore + RuntimeStore>(
    db: &T,
    record_type: &str,
    workflow_run_id: Option<Uuid>,
    external_item_id: Option<Uuid>,
) -> Result<Vec<Value>, SendableError> {
    db.fetch_automation_records(record_type.into(), workflow_run_id, external_item_id)
        .await
}

pub async fn put_idempotency_key<T: DeliveryStore>(
    db: &T,
    scope: String,
    key: String,
    result: Value,
) -> Result<Value, SendableError> {
    db.put_idempotency_key(scope, key, result).await
}

pub async fn fetch_idempotency_key<T: DeliveryStore>(
    db: &T,
    scope: String,
    key: String,
) -> Result<Option<Value>, SendableError> {
    db.fetch_idempotency_key(scope, key).await
}

/// reserve an action node's idempotency key on behalf of the worker about to invoke its provider.
/// `lease_seconds` is the caller's own execution deadline: a reservation older than that is treated
/// as abandoned by a dead worker and taken over.
pub async fn claim_idempotency_key<T: DeliveryStore>(
    db: &T,
    scope: String,
    key: String,
    owner_node_run_id: Uuid,
    lease_seconds: i64,
) -> Result<runinator_models::orchestration::IdempotencyClaim, SendableError> {
    let now = Utc::now();
    let stale_before = now - Duration::seconds(lease_seconds.max(1));
    db.claim_idempotency_key(scope, key, owner_node_run_id, now, stale_before)
        .await
}

/// free an unfinished reservation after a non-success outcome, so a retry is not held off.
pub async fn release_idempotency_key<T: DeliveryStore>(
    db: &T,
    scope: String,
    key: String,
    owner_node_run_id: Uuid,
) -> Result<bool, SendableError> {
    db.release_idempotency_key(scope, key, owner_node_run_id)
        .await
}

/// record a completed execution against a reserved key so a redelivery replays it.
pub async fn complete_idempotency_key<T: DeliveryStore>(
    db: &T,
    scope: String,
    key: String,
    owner_node_run_id: Uuid,
    result: Value,
) -> Result<bool, SendableError> {
    db.complete_idempotency_key(scope, key, owner_node_run_id, result, Utc::now())
        .await
}

pub async fn fetch_gates<T: AutomationStore>(
    db: &T,
    workflow_run_id: Option<Uuid>,
    status: Option<String>,
) -> Result<Vec<Value>, SendableError> {
    db.fetch_gates(workflow_run_id, status).await
}

pub async fn fetch_gate<T: RuntimeStore>(
    db: &T,
    gate_id: Uuid,
) -> Result<Option<Value>, SendableError> {
    db.fetch_gate(gate_id).await
}

pub async fn delete_gate<T: AutomationStore>(db: &T, gate_id: Uuid) -> Result<bool, SendableError> {
    db.delete_gate(gate_id).await
}

pub async fn delete_automation_record<T: AutomationStore>(
    db: &T,
    record_type: &str,
    record_id: Uuid,
) -> Result<bool, SendableError> {
    db.delete_automation_record(record_type.to_string(), record_id)
        .await
}

pub async fn create_gate<T: RuntimeStore>(db: &T, record: Value) -> Result<Value, SendableError> {
    db.create_gate(record).await
}
