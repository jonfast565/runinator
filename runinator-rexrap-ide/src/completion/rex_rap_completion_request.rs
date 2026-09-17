#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RexRapCompletionRequest {
    pub source: String,
    pub cursor_byte: usize,
    #[serde(default)]
    pub providers: Vec<ProviderMetadata>,
    // known config/secret slots, used to complete `config.scope.name` / `secret.scope.name`.
    #[serde(default)]
    pub settings: Vec<SettingSummary>,
}
