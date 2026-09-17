#[allow(unused_imports)]
use super::*;

/// Broker-only token claims. Kept separate so an ordinary identity JWT can never acquire transport
/// authority by presenting an extra claim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaClaims {
    pub sub: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: String,
    pub rid: String,
}
