#[allow(unused_imports)]
use super::*;

/// full UI descriptor for one workflow trigger kind. trigger config lives in the untyped
/// `configuration` blob, so fields are plain `UiField`s (no `FieldLocation`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowTriggerKindMetadata {
    pub kind: WorkflowTriggerKind,
    pub label: String,
    pub icon: String,
    pub description: String,
    #[serde(default)]
    pub fields: Vec<UiField>,
    #[serde(default)]
    pub default_configuration: Value,
}
