#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceAttachment {
    #[serde(default)]
    pub follow_run: bool,
    #[serde(flatten)]
    pub reference: WorkspaceReference,
    #[serde(default)]
    pub access: WorkspaceAccess,
    #[serde(default)]
    pub create: bool,
    #[serde(default)]
    pub results: BTreeMap<String, Value>,
}
