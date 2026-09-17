#[allow(unused_imports)]
use super::*;

/// runtime auth configuration shared across handlers and the middleware.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub enabled: bool,
    /// primary signing secret: every freshly issued token is signed with this.
    pub jwt_secret: Vec<u8>,
    /// optional previous signing secret accepted on verify during a rotation overlap window. tokens
    /// are never signed with it; it only keeps pre-rotation tokens valid until they expire.
    pub jwt_secret_previous: Option<Vec<u8>>,
    pub access_ttl_secs: i64,
    pub refresh_ttl_secs: i64,
}
