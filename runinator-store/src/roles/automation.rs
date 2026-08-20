//! approval/gate records and the audit log — the human- and policy-facing side of a run.
//!
//! one of the role traits `DatabaseImpl` composes. bound on this directly when a caller only
//! needs this slice of the store.

use std::future::Future;

use uuid::Uuid;

use runinator_models::errors::SendableError;
use runinator_models::value::Value;

// re-exported here so callers that reach for the contract at its historical path
// (`runinator_database::interfaces::*`) can import both halves from one place.
pub use crate::runtime_store::RuntimeStore;

/// Core persistence operations for Runinator.
/// Approval/gate records and the audit log — the human- and policy-facing side of a run.
pub trait AutomationStore: Send + Sync + 'static {
    /// Fetch a single orchestration record by its identifier.
    fn fetch_automation_record(
        &self,
        record_type: String,
        record_id: Uuid,
    ) -> impl Future<Output = Result<Option<Value>, SendableError>> + Send;

    /// Fetch gate rows with optional run and status filters.
    fn fetch_gates(
        &self,
        workflow_run_id: Option<Uuid>,
        status: Option<String>,
    ) -> impl Future<Output = Result<Vec<Value>, SendableError>> + Send;

    /// Fetch audit-log rows, newest first, with optional actor and action filters.
    fn fetch_audit_log(
        &self,
        actor_id: Option<Uuid>,
        action: Option<String>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<Value>, SendableError>> + Send;

    /// Delete an orchestration record of a given type; returns true when a row was removed.
    fn delete_automation_record(
        &self,
        record_type: String,
        record_id: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Delete a gate row; returns true when a row was removed.
    fn delete_gate(
        &self,
        gate_id: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
}
