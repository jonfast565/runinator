#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize)]
pub struct DevPackInspectResult {
    pub path: String,
    pub files: Vec<DevPackFile>,
    pub workflows: Vec<WorkflowDefinition>,
    pub triggers: Vec<WorkflowTrigger>,
    pub settings_count: usize,
    // identities (no values) of the setting slots the pack would write on import.
    pub settings: Vec<SettingSummary>,
}
