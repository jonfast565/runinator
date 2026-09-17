#[allow(unused_imports)]
use super::*;

/// returned once on creation; `secret` is the only time the raw key is shown.
#[derive(Debug, Clone, Serialize)]
pub struct CreateApiKeyResponse {
    pub api_key: ApiKey,
    pub secret: String,
}
