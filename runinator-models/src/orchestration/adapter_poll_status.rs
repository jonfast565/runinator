#[allow(unused_imports)]
use super::*;

/// Durable scheduling and diagnostic state for one polling adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterPollStatus {
    pub adapter_id: Uuid,
    pub revision: i64,
    #[serde(default)]
    pub checkpoint: Value,
    pub next_poll_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_until: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Frozen selector used when profile-backed polling is dispatched to workers.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub required_labels: BTreeMap<String, String>,
    /// Number of currently live workers satisfying `required_labels`.
    #[serde(default)]
    pub matching_worker_count: i64,
    /// Actionable scheduling diagnosis, present when polling cannot currently be dispatched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_diagnostic: Option<String>,
}
