#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerIngressSession {
    pub scope: ScopeRef,
    pub mode: BrokerIngressSessionMode,
    pub updated_by: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
    /// An inspector is a client-owned, renewable lease rather than a sticky server setting. This
    /// lets the engine stop recording/holding ingress shortly after the inspecting page closes or
    /// loses its connection.
    pub expires_at: DateTime<Utc>,
}
