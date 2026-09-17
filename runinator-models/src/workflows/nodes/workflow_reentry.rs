#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct WorkflowReentry {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub max_visits: i64,
    #[serde(default)]
    pub on_exhausted: Option<WorkflowNodeRef>,
}
