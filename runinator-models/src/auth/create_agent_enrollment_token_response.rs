#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAgentEnrollmentTokenResponse {
    pub enrollment_token: AgentEnrollmentToken,
    /// shown exactly once.
    pub token: String,
}
