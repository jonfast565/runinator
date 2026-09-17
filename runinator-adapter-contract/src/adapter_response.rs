#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterResponse {
    pub verified: bool,
    #[serde(default)]
    pub events: Vec<NormalizedAdapterEvent>,
    #[serde(default)]
    pub errors: Vec<String>,
    /// optional adapter-defined JSON response returned immediately after verification. this keeps
    /// webhook handshakes inside the adapter instead of teaching the HTTP handler vendor rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub immediate_response: Option<AdapterImmediateResponse>,
}

impl AdapterResponse {
    pub fn rejected(error: impl Into<String>) -> Self {
        Self {
            verified: false,
            events: Vec::new(),
            errors: vec![error.into()],
            immediate_response: None,
        }
    }
}
