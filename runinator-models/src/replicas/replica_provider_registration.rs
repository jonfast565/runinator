#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaProviderRegistration {
    pub replica_id: Uuid,
    pub provider_name: String,
    pub provider: ProviderMetadata,
    pub first_registered_at: DateTime<Utc>,
    pub last_registered_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
}
