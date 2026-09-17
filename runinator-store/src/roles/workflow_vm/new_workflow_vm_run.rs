#[allow(unused_imports)]
use super::*;

/// Everything needed to freeze a new VM-backed workflow run in one transaction.
#[derive(Debug, Clone)]
pub struct NewWorkflowVmRun {
    pub requested_run_id: Option<Uuid>,
    /// Verified replay-prefix values, installed atomically into a fresh root identity.
    pub replay_seed: Option<WorkflowReplaySeed>,
    pub workflow_id: Uuid,
    pub workflow_snapshot: WorkflowDefinition,
    pub parameters: Value,
    /// Eager configuration snapshot exposed to bytecode as the `config` local. Resolving it when
    /// the run starts makes retries and resumed continuations independent of later setting edits.
    pub config: Value,
    pub state: Value,
    pub name: Option<String>,
    pub provenance: WorkflowRunProvenance,
    /// Owning pipeline run when this is a pipeline member. This is persisted with the run row
    /// inside the same transaction as the module, root continuation, and first journal entry.
    pub pipeline_run_id: Option<Uuid>,
    /// Pipeline member attempt to bind to the new run in that same transaction.
    pub pipeline_member_attempt_id: Option<Uuid>,
    pub module: WorkflowModule,
    /// Initial bytecode location. Zero starts normally; replay uses a source-map boundary.
    pub instruction_pointer: usize,
}
