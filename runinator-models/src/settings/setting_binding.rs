#[allow(unused_imports)]
use super::*;

/// A workflow's durable dependency on a config or secret value. The UUID is authoritative while
/// the authored scope/key remains available for decompilation and run-input aliasing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingBinding {
    pub kind: SettingKind,
    pub reference: ArtifactRef,
}
