#[allow(unused_imports)]
use super::*;

/// the active-org context returned after a switch, with a re-issued access token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgContextResponse {
    pub access_token: String,
    /// access-token lifetime in seconds.
    pub expires_in: i64,
    pub org: Organization,
    pub role: OrgRole,
}
