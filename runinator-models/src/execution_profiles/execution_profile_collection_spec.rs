#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfileCollectionSpec {
    #[serde(default = "default_spec_version")]
    pub version: u32,
    #[serde(default)]
    pub probe: Option<ExecutionProfileCommand>,
    #[serde(default)]
    pub refresh: Option<ExecutionProfileCommand>,
    pub sources: Vec<ExecutionProfileSource>,
}

impl Default for ExecutionProfileCollectionSpec {
    fn default() -> Self {
        Self {
            version: default_spec_version(),
            probe: None,
            refresh: None,
            sources: Vec::new(),
        }
    }
}
