#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RexRapHoverRequest {
    pub source: String,
    pub cursor_byte: usize,
    #[serde(default)]
    pub providers: Vec<ProviderMetadata>,
    #[serde(default)]
    pub settings: Vec<SettingSummary>,
}
