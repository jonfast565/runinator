#[allow(unused_imports)]
use super::*;

/// a [`DebugVerb`] addressed to a specific workflow run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugCommand {
    pub workflow_run_id: Uuid,
    #[serde(flatten)]
    pub verb: DebugVerb,
}

impl DebugCommand {
    pub fn new(workflow_run_id: Uuid, verb: DebugVerb) -> Self {
        Self {
            workflow_run_id,
            verb,
        }
    }
}
