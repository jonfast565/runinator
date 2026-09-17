#[allow(unused_imports)]
use super::*;

/// A definition snapshot and the bytecode compiled from exactly that snapshot. Schedule claims
/// receive these together so an edit racing the claim cannot pair old bytecode with new JSON.
#[derive(Debug, Clone)]
pub struct ScheduledWorkflowVm {
    pub snapshot: WorkflowDefinition,
    pub module: WorkflowModule,
}
