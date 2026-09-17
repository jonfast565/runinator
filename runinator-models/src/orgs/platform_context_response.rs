#[allow(unused_imports)]
use super::*;

/// the platform-scope context returned after leaving an active organization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformContextResponse {
    pub access_token: String,
    /// access-token lifetime in seconds.
    pub expires_in: i64,
}
