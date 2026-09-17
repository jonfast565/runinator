#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerIngressRecord {
    pub id: Uuid,
    pub scope: ScopeRef,
    pub delivery_id: Uuid,
    pub dedupe_key: String,
    pub command_kind: String,
    pub command: Value,
    pub state: IngressControlState,
    pub reviewed_by: Option<Uuid>,
    pub last_error: Option<String>,
    pub received_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}
