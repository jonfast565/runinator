#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct StoredSecretEnvelope {
    pub(super) value: String,
    pub(super) expires_at: Option<DateTime<Utc>>,
}
