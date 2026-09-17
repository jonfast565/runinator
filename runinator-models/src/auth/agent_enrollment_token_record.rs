#[allow(unused_imports)]
use super::*;

/// persistence form; the HMAC secret is sealed with the credential cipher before storage.
#[derive(Debug, Clone)]
pub struct AgentEnrollmentTokenRecord {
    pub token: AgentEnrollmentToken,
    pub sealed_secret: Vec<u8>,
}
