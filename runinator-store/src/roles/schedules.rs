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

/// Core persistence operations for Runinator.
/// When work fires: workflow and pipeline triggers, firing claims, freeze windows, and backfill.
mod scheduled_workflow_vm;
pub use scheduled_workflow_vm::ScheduledWorkflowVm;

mod schedule_store;
pub use schedule_store::ScheduleStore;
