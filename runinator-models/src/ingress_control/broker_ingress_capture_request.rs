#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerIngressCaptureRequest {
    pub scope: ScopeRef,
    pub delivery_id: Uuid,
    pub dedupe_key: String,
    pub command_kind: String,
    pub command: Value,
    pub hold: bool,
    pub received_at: DateTime<Utc>,
    pub capacity: i64,
}
