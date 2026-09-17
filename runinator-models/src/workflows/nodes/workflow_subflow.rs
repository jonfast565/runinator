#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct WorkflowSubflow {
    /// The durable target selected at authoring/import time. The legacy `subflow_id` field on the
    /// node is kept in sync for wire compatibility; new definitions should use this full reference
    /// so the originally authored path can survive a namespace move.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<ArtifactRef>,
    #[serde(default)]
    pub workflow_name: Option<String>,
    /// Temporary source-level `@revision(N)` selector. Pack/import resolution replaces this with
    /// `target.revision_pin` before the definition is persisted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    #[serde(default)]
    pub run_name: Option<Value>,
    #[serde(default)]
    pub reuse_open_run: bool,
    #[serde(default, rename = "type")]
    pub subflow_type: WorkflowSubflowType,
}

impl WorkflowSubflow {
    /// Build the compatibility UUID field from the canonical artifact reference, rejecting a
    /// non-workflow artifact before it can enter the workflow runtime.
    pub fn target_workflow_id(&self) -> Option<Uuid> {
        self.target
            .as_ref()
            .filter(|reference| reference.kind == ArtifactKind::Workflow)
            .map(|reference| reference.id)
    }

    /// Retain a best-effort authored path for old JSON that carried only `workflow_name`.
    pub fn authored_path(&self) -> Option<ArtifactPath> {
        self.target
            .as_ref()
            .and_then(|reference| reference.authored_path.clone())
            .or_else(|| {
                self.workflow_name
                    .as_deref()
                    .filter(|name| !name.trim().is_empty())
                    .map(ArtifactPath::from_qualified)
            })
    }
}
