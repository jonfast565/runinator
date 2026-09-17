#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct OutputParameters {
    pub event_type: Option<String>,
    pub data: WorkflowExpression,
    /// artifact declarations: name/source pairs promoted to run-level by this output node.
    pub items: Vec<ArtifactItem>,
}
