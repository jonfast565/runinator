#[allow(unused_imports)]
use super::*;

/// One run-level artifact declaration attached to an output instruction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowOutputArtifact {
    pub name: String,
    pub source: InvocationModule,
}
