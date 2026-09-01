//! when work fires: workflow and pipeline triggers, firing claims, freeze windows, and backfill.
//!
//! one of the role traits `DatabaseImpl` composes. bound on this directly when a caller only
//! needs this slice of the store.

use std::{collections::HashMap, future::Future};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use runinator_models::{
    errors::SendableError,
    pipelines::{PipelineRun, PipelineTrigger},
    schedules::{
        BackfillRequest, BackfillResponse, CalendarSubscription, FreezeWindow,
        NewCalendarSubscriptionRecord, NewFreezeWindow, TriggerFiringBatch,
    },
    workflow_vm::WorkflowModule,
    workflows::{WorkflowDefinition, WorkflowRun, WorkflowTrigger},
};

/// A definition snapshot and the bytecode compiled from exactly that snapshot. Schedule claims
/// receive these together so an edit racing the claim cannot pair old bytecode with new JSON.
#[derive(Debug, Clone)]
pub struct ScheduledWorkflowVm {
    pub snapshot: WorkflowDefinition,
    pub module: WorkflowModule,
}

/// Core persistence operations for Runinator.
/// When work fires: workflow and pipeline triggers, firing claims, freeze windows, and backfill.
pub trait ScheduleStore: Send + Sync + 'static {
    /// Create or update a workflow trigger.
    fn upsert_workflow_trigger(
        &self,
        trigger: &WorkflowTrigger,
    ) -> impl Future<Output = Result<WorkflowTrigger, SendableError>> + Send;

    /// Fetch a workflow trigger by identifier.
    fn fetch_workflow_trigger(
        &self,
        trigger_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkflowTrigger>, SendableError>> + Send;

    /// Delete a workflow trigger.
    fn delete_workflow_trigger(
        &self,
        trigger_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Create or update a pipeline-level trigger.
    fn upsert_pipeline_trigger(
        &self,
        trigger: &PipelineTrigger,
    ) -> impl Future<Output = Result<PipelineTrigger, SendableError>> + Send;

    /// Fetch all triggers owned by a pipeline.
    fn fetch_pipeline_triggers(
        &self,
        pipeline_id: Uuid,
    ) -> impl Future<Output = Result<Vec<PipelineTrigger>, SendableError>> + Send;

    /// Fetch a pipeline trigger by identifier.
    fn fetch_pipeline_trigger(
        &self,
        trigger_id: Uuid,
    ) -> impl Future<Output = Result<Option<PipelineTrigger>, SendableError>> + Send;

    /// Delete a pipeline trigger.
    fn delete_pipeline_trigger(
        &self,
        trigger_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Atomically fire due cron pipeline triggers and return the pipeline runs created by this claim
    /// (status `queued`; entry members are started by the repository layer).
    fn claim_due_pipeline_trigger_firings(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<PipelineRun>, SendableError>> + Send;

    /// Fetch enabled triggers that should fire at or before the provided instant.
    fn fetch_due_workflow_triggers(
        &self,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<WorkflowTrigger>, SendableError>> + Send;

    /// Update the next execution instant for a workflow trigger.
    fn update_workflow_trigger_next_execution(
        &self,
        trigger_id: Uuid,
        next_execution: Option<DateTime<Utc>>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Atomically fire due cron triggers, honouring each workflow's concurrency policy, each
    /// trigger's catch-up policy, and any active freeze window. Returns the runs created plus the
    /// runs a `cancel_previous` policy set terminal, which the caller still has to tell workers about.
    fn claim_due_workflow_trigger_firings(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        limit: i64,
        modules: HashMap<Uuid, ScheduledWorkflowVm>,
    ) -> impl Future<Output = Result<TriggerFiringBatch<WorkflowRun>, SendableError>> + Send;

    /// Replay the cron slots of one trigger across a past time range. Slots that already have a
    /// firing recorded are left alone, so a backfill can never double-run a slot the loop fired.
    fn backfill_workflow_trigger(
        &self,
        trigger_id: Uuid,
        request: &BackfillRequest,
        workflow_vm: ScheduledWorkflowVm,
    ) -> impl Future<Output = Result<(BackfillResponse, Vec<WorkflowRun>), SendableError>> + Send;

    /// List freeze windows, optionally narrowed to one org's windows plus the platform-wide ones.
    fn fetch_freeze_windows(
        &self,
        org_id: Option<Uuid>,
    ) -> impl Future<Output = Result<Vec<FreezeWindow>, SendableError>> + Send;

    fn fetch_freeze_window(
        &self,
        window_id: Uuid,
    ) -> impl Future<Output = Result<Option<FreezeWindow>, SendableError>> + Send;

    /// The freeze windows in effect at `now`, used to explain why a schedule is not firing.
    fn fetch_active_freeze_windows(
        &self,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<FreezeWindow>, SendableError>> + Send;

    /// Advance recurring freeze rows to the interval active at `now`, or their next interval.
    fn refresh_freeze_windows(
        &self,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    fn create_freeze_window(
        &self,
        window: &NewFreezeWindow,
    ) -> impl Future<Output = Result<FreezeWindow, SendableError>> + Send;

    fn update_freeze_window(
        &self,
        window_id: Uuid,
        window: &NewFreezeWindow,
    ) -> impl Future<Output = Result<Option<FreezeWindow>, SendableError>> + Send;

    fn delete_freeze_window(
        &self,
        window_id: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    fn create_calendar_subscription(
        &self,
        record: &NewCalendarSubscriptionRecord,
    ) -> impl Future<Output = Result<CalendarSubscription, SendableError>> + Send;

    fn fetch_calendar_subscription_by_hash(
        &self,
        token_hash: String,
    ) -> impl Future<Output = Result<Option<CalendarSubscription>, SendableError>> + Send;

    fn delete_calendar_subscription(
        &self,
        subscription_id: Uuid,
        principal_id: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
}
