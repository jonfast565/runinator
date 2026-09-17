#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollAgentRequest {
    pub token_id: String,
    pub request_body: AgentEnrollmentRequestBody,
    /// base64url-no-pad HMAC-SHA256 over the canonical JSON request body.
    pub proof: String,
}

impl Validate for EnrollAgentRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("token_id", &self.token_id)?;
        self.request_body.validate()?;
        required_text("proof", &self.proof, 1024)
    }
}
