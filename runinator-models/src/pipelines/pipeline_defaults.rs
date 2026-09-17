#[allow(unused_imports)]
use super::*;

/// editable pipeline-level defaults applied when authoring links inside a pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<Value>,
    #[serde(default)]
    pub on_step_failure: PipelineFailurePolicy,
    #[serde(default = "default_true")]
    pub links_enabled_by_default: bool,
    #[serde(default)]
    pub default_parameters: Value,
    #[serde(default)]
    pub max_chain_depth: Option<u32>,
    /// the failure mode copied onto a member that omits one during import.
    #[serde(default)]
    pub default_failure_mode: PipelineMemberFailureMode,
}

impl Default for PipelineDefaults {
    fn default() -> Self {
        PipelineDefaults {
            workspace: None,
            on_step_failure: PipelineFailurePolicy::default(),
            links_enabled_by_default: true,
            default_parameters: Value::default(),
            max_chain_depth: None,
            default_failure_mode: PipelineMemberFailureMode::default(),
        }
    }
}

impl PartialEq for PipelineDefaults {
    fn eq(&self, other: &Self) -> bool {
        self.workspace == other.workspace
            && self.on_step_failure == other.on_step_failure
            && self.links_enabled_by_default == other.links_enabled_by_default
            && self.default_parameters == other.default_parameters
            && self.max_chain_depth == other.max_chain_depth
            && self.default_failure_mode == other.default_failure_mode
    }
}
