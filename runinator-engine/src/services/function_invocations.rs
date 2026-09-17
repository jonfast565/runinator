//! application service for invoking packaged-function adapter workflows.

use std::sync::Arc;

use runinator_broker_core::{Broker, EmbeddedEngineSignals, UiEventPublisher, emit_workflow_run};
use runinator_models::{
    errors::SendableError,
    functions::{FunctionExport, FunctionPackage, FunctionVersion, FunctionVersionRef},
    replicas::WorkflowRunProvenance,
    value::Value,
    workflows::WorkflowRun,
};
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, DeliveryStore, FunctionStore, WorkflowVmStore},
};
use uuid::Uuid;

use crate::repository;

/// A resolved packaged-function export and the immutable adapter workflow that executes it.
mod resolved_function_invocation;
pub use resolved_function_invocation::ResolvedFunctionInvocation;

mod function_invocations;
pub use function_invocations::FunctionInvocations;
