//! Durable host contract for the compiled workflow VM.
//!
//! The important operation is [`WorkflowVmStore::suspend_on_effect`]: it updates the frozen
//! continuation, records its uniquely-identified effect and queues its external delivery in one
//! transaction. Implementations must make a duplicate `(continuation_id, sequence)` a no-op.

use std::future::Future;

use chrono::{DateTime, Utc};
use runinator_comm::{EffectCommand, EffectDispatchRecord};
use runinator_models::{
    errors::SendableError,
    replicas::WorkflowRunProvenance,
    value::Value,
    workflow_vm::{
        WorkflowContinuation, WorkflowEffect, WorkflowEffectOutputEvent, WorkflowEffectStatus,
        WorkflowInterruptOutcome, WorkflowJournalEntry, WorkflowJournalRecord, WorkflowModule,
        WorkflowPendingInterrupt,
    },
    workflows::{WorkflowDefinition, WorkflowRun, WorkflowStatus},
};
use uuid::Uuid;

/// Persistence used only by the compiled workflow runtime and its durable host.

/// An effect completion and its optional portable workspace snapshot.
mod workflow_timer_interrupt;
pub use workflow_timer_interrupt::WorkflowTimerInterrupt;

mod new_workflow_vm_run;
pub use new_workflow_vm_run::NewWorkflowVmRun;

mod workflow_replay_seed;
pub use workflow_replay_seed::WorkflowReplaySeed;

mod workflow_vm_store;
pub use workflow_vm_store::WorkflowVmStore;

mod workspace_effect_settlement;
pub use workspace_effect_settlement::WorkspaceEffectSettlement;
