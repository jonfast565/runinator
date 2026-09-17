#[allow(unused_imports)]
use super::*;

/// a portable, id-free pipeline declaration compiled from a `.rexrapp` file. members and links use
/// canonical workflow paths; the web service resolves those paths to ids and persists one atomic graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineSpec {
    pub name: String,
    /// Stable key used to find this logical pipeline across display-name edits and namespace moves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub defaults: PipelineDefaults,
    #[serde(default)]
    pub members: Vec<PipelineMemberSpec>,
    #[serde(default)]
    pub links: Vec<PipelineLinkSpec>,
    #[serde(default)]
    pub joins: Vec<PipelineJoinSpec>,
    #[serde(default)]
    pub concurrency: WorkflowConcurrency,
    /// Portable pipeline metadata authored with the pack.  Importers add their managed markers
    /// without replacing this object, so generic policies can travel with the declaration.
    #[serde(default)]
    pub metadata: Value,
    /// pipeline-level triggers (cron / manual / chained) declared in the `.rexrapp` header. materialized
    /// on import as managed `pipeline_triggers` reconciled by pipeline id.
    #[serde(default)]
    pub triggers: Vec<PipelineTriggerSpec>,
}
