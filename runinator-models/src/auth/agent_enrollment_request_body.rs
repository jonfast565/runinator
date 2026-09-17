#[allow(unused_imports)]
use super::*;

/// body authenticated by the enrollment HMAC. labels are a request only; the server rejects any
/// value outside the token's authorized label set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEnrollmentRequestBody {
    pub instance_id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

impl Validate for AgentEnrollmentRequestBody {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("request_body.instance_id", &self.instance_id)?;
        optional_text(
            "request_body.display_name",
            self.display_name.as_deref(),
            SHORT_TEXT_MAX,
        )?;
        string_map("request_body.labels", &self.labels, 64)
    }
}
