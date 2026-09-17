#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewWorkspaceLease {
    pub id: Uuid,
    pub admission_id: Uuid,
    pub generation: i64,
    pub scope: String,
    pub attempt: i64,
    pub worker_instance_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_replica_id: Option<Uuid>,
    pub local_key: String,
    #[serde(default)]
    pub requirements: Value,
    pub leased_until: DateTime<Utc>,
}
