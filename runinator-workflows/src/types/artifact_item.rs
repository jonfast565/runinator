#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct ArtifactItem {
    pub name: String,
    /// value-ref that resolves to an artifact descriptor (or array of them) at runtime.
    pub source: WorkflowExpression,
}
