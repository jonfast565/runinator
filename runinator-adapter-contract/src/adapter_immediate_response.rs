#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterImmediateResponse {
    pub status: u16,
    #[serde(default)]
    pub body: Value,
}
