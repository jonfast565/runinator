#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct OrchestrationPhaseDecl {
    pub member: String,
    pub mappings: Vec<(String, String)>,
    pub workspace: Option<OrchestrationWorkspaceDecl>,
    pub span: Span,
}
