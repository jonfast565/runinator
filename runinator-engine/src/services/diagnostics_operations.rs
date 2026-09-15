//! application service for durable runtime diagnostics.

use std::sync::Arc;

use runinator_models::{
    diagnostics::{RuntimeLogPage, RuntimeLogQuery, RuntimeLogRecord},
    errors::SendableError,
};
use runinator_store::{
    RuntimeStore,
    roles::{DeliveryStore, WorkflowVmStore},
};
use uuid::Uuid;

/// Coordinates runtime-log persistence and queries outside the HTTP transport.
pub struct DiagnosticsOperations<T> {
    db: Arc<T>,
}

impl<T> DiagnosticsOperations<T> {
    pub fn new(db: Arc<T>) -> Self {
        Self { db }
    }
}

impl<T: DeliveryStore> DiagnosticsOperations<T> {
    pub async fn fetch(
        &self,
        query: &RuntimeLogQuery,
        correlated_run: Option<Uuid>,
        limit: i64,
        retention_seconds: i64,
    ) -> Result<RuntimeLogPage, SendableError> {
        crate::repository::fetch_runtime_logs(
            self.db.as_ref(),
            query,
            correlated_run,
            limit,
            retention_seconds,
        )
        .await
    }
}

impl<T: DeliveryStore + RuntimeStore + WorkflowVmStore> DiagnosticsOperations<T> {
    pub async fn record(&self, mut records: Vec<RuntimeLogRecord>) -> Result<(), SendableError> {
        for record in &mut records {
            if let Some(effect_id) = record.effect_id {
                let effect = self
                    .db
                    .fetch_workflow_effect(effect_id)
                    .await?
                    .ok_or_else(|| {
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "runtime diagnostic references an unknown workflow effect",
                        )) as SendableError
                    })?;
                if record
                    .workflow_run_id
                    .is_some_and(|run_id| run_id != effect.workflow_run_id)
                {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "runtime diagnostic effect and workflow run do not match",
                    )));
                }
                record.workflow_run_id = Some(effect.workflow_run_id);
                continue;
            }
            if let Some(run_id) = record.workflow_run_id
                && self.db.fetch_workflow_run(run_id).await?.is_none()
            {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "runtime diagnostic references an unknown workflow run",
                )));
            }
        }
        crate::repository::record_runtime_logs(self.db.as_ref(), records).await
    }
}

impl<T: WorkflowVmStore> DiagnosticsOperations<T> {
    pub async fn run_for_effect(&self, effect_id: Uuid) -> Result<Option<Uuid>, SendableError> {
        crate::repository::diagnostic_run_for_effect(self.db.as_ref(), effect_id).await
    }
}
