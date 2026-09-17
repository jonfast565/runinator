#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct WorkflowWait {
    #[serde(default)]
    pub seconds: Option<WorkflowWaitSeconds>,
    #[serde(default)]
    pub until_status: Option<String>,
    #[serde(default)]
    pub initial_status: Option<String>,
}
