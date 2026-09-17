#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAgentEnrollmentTokenRequest {
    /// Redemption deadline in seconds and, for timed enrollment, the issued credential lifetime.
    pub ttl_seconds: u64,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    pub service_url: String,
    /// override for deployments whose LAN announcement URL differs from the public enrollment URL.
    #[serde(default)]
    pub cluster_id: Option<Uuid>,
    #[serde(default)]
    pub spki_pin: Option<String>,
    /// Issue a non-expiring machine credential. Timed access remains the default.
    #[serde(default)]
    pub permanent: bool,
}

impl Validate for CreateAgentEnrollmentTokenRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if !(1..=86_400).contains(&self.ttl_seconds) {
            return Err(ValidationError::new(
                "ttl_seconds",
                "must be between 1 and 86400",
            ));
        }
        http_url("service_url", &self.service_url)?;
        string_map("labels", &self.labels, 64)?;
        optional_text("spki_pin", self.spki_pin.as_deref(), SHORT_TEXT_MAX)
    }
}
