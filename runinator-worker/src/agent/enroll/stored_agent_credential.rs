#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct StoredAgentCredential {
    pub(super) service_url: String,
    pub(super) api_key: String,
    pub(super) instance_id: String,
    #[serde(default)]
    pub(super) labels: BTreeMap<String, String>,
    #[serde(default)]
    pub(super) cluster_id: Option<uuid::Uuid>,
}
