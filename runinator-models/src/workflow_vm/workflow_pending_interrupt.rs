#[allow(unused_imports)]
use super::*;

/// An interrupt asked for out of band, recorded on the thread it targets until that thread reaches
/// a safe point. `External` and `OrphanSignal` arrive this way; the sources the VM detects for
/// itself never take this route.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowPendingInterrupt {
    pub id: Uuid,
    pub source: InterruptSource,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub payload: Value,
}
