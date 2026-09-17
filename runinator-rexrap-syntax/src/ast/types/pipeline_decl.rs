#[allow(unused_imports)]
use super::*;

/// a `pipeline "Name" { ... }` block parsed from a `.rexrapp` file. `on_failure` holds the raw policy
/// keyword (`halt`/`continue`) or `None`; lowering maps the string fields to the model enums.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineDecl {
    pub workspace: Option<super::super::Expr>,
    pub name: String,
    pub key: Option<String>,
    pub namespace: Option<String>,
    pub description: Option<String>,
    pub on_failure: Option<String>,
    pub max_depth: Option<u32>,
    pub links_enabled_by_default: Option<bool>,
    pub default_parameters: Option<Expr>,
    pub default_failure_mode: Option<String>,
    pub metadata: Option<Expr>,
    pub members: Vec<PipelineMemberDecl>,
    pub links: Vec<PipelineLinkDecl>,
    pub joins: Vec<PipelineJoinDecl>,
    pub concurrency: Option<super::super::ConcurrencyDecl>,
    pub ingress: Option<super::super::IngressDecl>,
    pub orchestration: Option<OrchestrationDecl>,
    pub triggers: Vec<PipelineTriggerDecl>,
    pub span: Span,
}
