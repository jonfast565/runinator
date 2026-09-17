#[allow(unused_imports)]
use super::*;

/// Durable machine identity created by redeeming an agent enrollment token. Timed credentials
/// expire with their enrollment grant; permanent credentials remain usable until the machine is
/// invalidated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMachineEnrollment {
    pub machine_id: Uuid,
    pub instance_id: String,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    pub permanent: bool,
    pub disabled: bool,
    #[serde(default)]
    pub credential_count: usize,
    #[serde(default)]
    pub active_credential_count: usize,
    #[serde(default)]
    pub enrolled_by: Option<Uuid>,
    pub enrolled_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub last_used_at: Option<DateTime<Utc>>,
}
