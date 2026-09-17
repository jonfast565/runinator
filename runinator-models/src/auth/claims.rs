#[allow(unused_imports)]
use super::*;

/// JWT access-token claims.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// subject: the user id.
    pub sub: String,
    /// backing refresh-session id; access is revoked immediately with the session.
    pub sid: String,
    /// issued-at (unix seconds).
    pub iat: i64,
    /// expiry (unix seconds).
    pub exp: i64,
    /// token id, for future revocation lists.
    pub jti: String,
    /// active organization for this token, when the user has switched into one. absent on tokens
    /// minted before an org was selected, and on service/replica tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,
}
