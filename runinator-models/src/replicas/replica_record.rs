#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaRecord {
    pub replica_id: Uuid,
    pub replica_type: ReplicaKind,
    pub instance_id: String,
    pub runtime_id: String,
    pub status: ReplicaStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub attributes: Value,
    pub first_seen_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline_at: Option<DateTime<Utc>>,
    /// Operator-enforced end of this activation. A kicked runtime cannot heartbeat or re-register;
    /// the enrolled machine may start a fresh activation with a new replica id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kicked_at: Option<DateTime<Utc>>,
    /// the identity that registered this replica, captured once at insert and never reassigned by
    /// later heartbeats/upserts. lets a lower-trust external caller (e.g. a desktop-agent connecting
    /// through the WS broker relay) be checked against the replica_id/labels it presents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registered_by_principal_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registered_by_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registered_by_org_id: Option<Uuid>,
}
