#[allow(unused_imports)]
use super::*;

pub struct PackImportRequest<'a> {
    pub contract_override_reason: Option<String>,
    pub workflows: WorkflowBundle,
    pub settings: Option<&'a SettingsBundle>,
    pub pipelines: Option<&'a PipelineBundle>,
    pub functions: &'a [NewFunctionVersion],
    pub artifacts: &'a [FunctionArtifact],
    pub import_org: Option<Uuid>,
    pub owner: ScopeRef,
    pub created_by: Option<Uuid>,
    pub overwrite: bool,
}
