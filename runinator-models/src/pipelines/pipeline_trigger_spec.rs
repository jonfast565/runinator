#[allow(unused_imports)]
use super::*;

/// a portable, id-free pipeline trigger declaration compiled from a `.rexrapp` header. `configuration`
/// carries kind-specific data (cron: `{cron, parameters}`; chained: `{on, source_workflow |
/// source_pipeline, source_workflow_id | source_pipeline_id, parameters}`); manual triggers carry
/// no schedule. The path is authored for diagnostics; the resolved UUID is authoritative at runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineTriggerSpec {
    pub kind: WorkflowTriggerKind,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub configuration: Value,
}
