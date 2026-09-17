#[allow(unused_imports)]
use super::*;

/// a member workflow declared in a `.rexrapp` pipeline, by canonical `namespace.key` path.
/// `failure_mode` is `None` when the
/// member declares no `on_failure` of its own, meaning it takes the pipeline's
/// [`PipelineDefaults::default_failure_mode`] at import.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineMemberSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<Value>,
    pub name: String,
    #[serde(default)]
    pub failure_mode: Option<PipelineMemberFailureMode>,
}

impl From<&str> for PipelineMemberSpec {
    fn from(name: &str) -> Self {
        PipelineMemberSpec {
            workspace: None,
            name: name.to_string(),
            failure_mode: None,
        }
    }
}

impl From<String> for PipelineMemberSpec {
    fn from(name: String) -> Self {
        PipelineMemberSpec {
            workspace: None,
            name,
            failure_mode: None,
        }
    }
}
