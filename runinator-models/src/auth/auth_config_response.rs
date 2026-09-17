#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfigResponse {
    pub enabled: bool,
}
