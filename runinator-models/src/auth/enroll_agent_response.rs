#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollAgentResponse {
    pub api_key: String,
    pub service_url: String,
    /// Absent for a permanent machine enrollment.
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}
