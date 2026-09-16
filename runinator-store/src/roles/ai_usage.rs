//! durable AI token and cost records.

use std::future::Future;

use chrono::{DateTime, Utc};
use runinator_models::{ai_usage::AiUsageRecord, errors::SendableError};
use uuid::Uuid;

pub trait AiUsageStore: Send + Sync + 'static {
    /// Insert one terminal-result usage record. Returns false when the event was already recorded.
    fn insert_ai_usage(
        &self,
        record: AiUsageRecord,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    fn fetch_ai_usage_for_run(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<AiUsageRecord>, SendableError>> + Send;

    fn fetch_ai_usage_for_workflow(
        &self,
        workflow_id: Uuid,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> impl Future<Output = Result<Vec<AiUsageRecord>, SendableError>> + Send;
}
