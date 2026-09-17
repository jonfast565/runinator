//! application service for commands that create or change workflow runs.
//!
//! The HTTP surface supplies request parsing and authorization. This service owns the durable
//! command, its broker side effect, and the broker-backed UI notification so those three actions
//! cannot drift between transports.

use std::collections::BTreeSet;
use std::sync::Arc;

use runinator_broker_core::{Broker, EmbeddedEngineSignals, UiEventPublisher, emit_workflow_run};
use runinator_models::{
    auth::ResourceType,
    errors::SendableError,
    files::{FileScope, referenced_file_ids},
    interrupt::InterruptSource,
    replicas::WorkflowRunProvenance,
    value::Value,
    web::TaskResponse,
    workflow_state::WorkflowExecutionState,
    workflows::{WorkflowRun, WorkflowStatus},
};
use runinator_store::{
    RuntimeStore,
    roles::{
        AiUsageStore, FileStore, OrchestrationStore, RunStore, ScheduleStore, WorkflowVmStore,
    },
};
use uuid::Uuid;

use crate::repository;

mod run_operations;
pub use run_operations::RunOperations;

mod create_workflow_run_request;
pub use create_workflow_run_request::CreateWorkflowRunRequest;
